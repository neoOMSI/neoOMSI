//! The object editor's keys, mouse and wheel, and its edits sent to the LAN players.

use super::*;

impl App {
    pub(crate) fn toggle_editor(&mut self) {
        if self.editor.is_none()
            && self
                .lan
                .as_ref()
                .map(|l| l.role == ::network::Role::Client)
                .unwrap_or(false)
        {
            self.service_msg = Some(("In a LAN session only the host edits the map".into(), 3.0));
            return;
        }
        if self.editor.take().is_some() {
            self.editor_drag = false;
            self.service_msg = Some((
                "Object editor off (unsaved changes stay until the end of the session)".into(),
                3.0,
            ));
            return;
        }
        let ed = crate::editor::Editor::default();
        let msg = self
            .world
            .as_ref()
            .map(|w| ed.describe(w))
            .unwrap_or_default();
        self.editor = Some(ed);
        self.service_msg = Some((
            format!(
                "{msg} - click picks, drag moves, wheel turns (Shift: height), Delete, C copy, V variant, Backspace undo, PgUp/PgDn/F ground, [ ] brush, Ctrl+S save, Esc leave"
            ),
            10.0,
        ));
    }

    pub(crate) fn editor_key(&mut self, code: KeyCode) -> bool {
        let shift =
            self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight);
        let ctrl =
            self.keys.contains(&KeyCode::ControlLeft) || self.keys.contains(&KeyCode::ControlRight);
        let Some(cam) = self.camera.as_ref() else {
            return false;
        };
        let (eye, fwd, yaw) = (cam.position, cam.forward(), cam.yaw as f64);
        let Some(action) = crate::editor::action_for(code, shift, ctrl, yaw) else {
            return false;
        };
        let Some(world) = self.world.clone() else {
            return false;
        };
        let msg = match action {
            crate::editor::Action::Leave => {
                self.toggle_editor();
                return true;
            }
            crate::editor::Action::Pick => {
                let ed = self.editor.as_mut().unwrap();
                ed.pick(&world, eye, fwd);
                ed.describe(&world)
            }
            crate::editor::Action::NextPick => {
                let ed = self.editor.as_mut().unwrap();
                ed.next_pick();
                ed.describe(&world)
            }
            crate::editor::Action::Save => {
                let content = crate::startup::content_dir();
                let ed = self.editor.as_ref().unwrap();
                match content.map(|c| ed.save(&world, &self.args.map, &c, &self.args.root)) {
                    Some(Ok(files)) if files.is_empty() => "Nothing to save".to_string(),
                    Some(Ok(files)) => format!(
                        "Saved {} tile(s) to the content folder ({})",
                        files.len(),
                        files
                            .iter()
                            .filter_map(|f| f.file_name())
                            .map(|n| n.to_string_lossy())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    Some(Err(e)) => format!("Not saved: {e}"),
                    None => "Not saved: no content folder".to_string(),
                }
            }
            a @ (crate::editor::Action::Ground(_)
            | crate::editor::Action::Flatten
            | crate::editor::Action::Brush(_)) => {
                let at = crate::editor::Editor::aim(&world, eye, fwd);
                let (msg, tiles) = self.editor.as_mut().unwrap().ground(&world, at, &a);
                if !tiles.is_empty() {
                    log::info!("map editor: ground of tiles {tiles:?} at {at:?}");
                    world.forget_staged(&tiles);
                    if let (Some(st), Some(r), Some(scene)) = (
                        self.streamer.as_mut(),
                        self.renderer.as_ref(),
                        self.scene.as_mut(),
                    ) {
                        st.reload(r, scene, Some(&tiles), self.audio.as_ref());
                    }
                }
                msg
            }
            a => {
                let (Some(r), Some(scene)) = (self.renderer.as_ref(), self.scene.as_mut()) else {
                    return true;
                };
                match self.editor.as_mut().unwrap().apply(&world, r, scene, &a) {
                    Some(m) => m,
                    None => "Pick an object first (Enter)".to_string(),
                }
            }
        };
        log::info!("object editor: {msg}");
        self.service_msg = Some((msg, 5.0));
        self.editor_broadcast(false);
        true
    }

    pub(crate) fn editor_broadcast(&mut self, all: bool) {
        let (Some(ed), Some(w)) = (self.editor.as_ref(), self.world.as_ref()) else {
            if all {
                if let (Some(w), Some(l)) = (self.world.as_ref(), self.lan.as_mut()) {
                    if l.role == ::network::Role::Host {
                        let lines =
                            crate::editor::Editor::default().sync_lines(w, &self.args.root, true);
                        let ids: Vec<u32> = l
                            .peers()
                            .map(|p| p.pose.id)
                            .filter(|id| *id != l.my_id)
                            .collect();
                        for line in &lines {
                            for id in &ids {
                                l.command(*id, line);
                            }
                        }
                    }
                }
            }
            return;
        };
        let Some(l) = self.lan.as_mut() else { return };
        if l.role != ::network::Role::Host {
            return;
        }
        let lines = ed.sync_lines(w, &self.args.root, all);
        let ids: Vec<u32> = l
            .peers()
            .map(|p| p.pose.id)
            .filter(|id| *id != l.my_id)
            .collect();
        for line in &lines {
            if line.len() > ::network::MAX_CHAT {
                log::warn!("object editor: '{line}' is too long to send");
                continue;
            }
            for id in &ids {
                l.command(*id, line);
            }
        }
    }

    pub(crate) fn editor_mouse(&mut self, pressed: bool) -> bool {
        if self.editor.is_none() {
            return false;
        }
        if !pressed {
            if self.editor_drag {
                self.editor_drag = false;
                self.editor_broadcast(false);
            }
            return true;
        }
        let (Some(cam), Some(s), Some(world)) = (
            self.camera.as_ref(),
            self.surface.as_ref(),
            self.world.clone(),
        ) else {
            return true;
        };
        let (o, d) = cursor_ray_with_projection(
            cam,
            self.cursor.0,
            self.cursor.1,
            s.config.width as f32,
            s.config.height as f32,
            self.triple_screen_projection_active(),
        );
        let ed = self.editor.as_mut().unwrap();
        let on_added = ed
            .editing_added
            .and_then(|k| ed.added.get(k))
            .map(|a| {
                let p = a.base + a.moved - o;
                let along = p.dot(d.as_dvec3());
                along > 0.0 && (p - d.as_dvec3() * along).length() < 2.5
            })
            .unwrap_or(false);
        if !on_added {
            ed.pick(&world, o, d);
        }
        self.editor_drag = on_added || ed.selected.is_some();
        let msg = ed.describe(&world);
        self.service_msg = Some((msg, 5.0));
        true
    }

    pub(crate) fn editor_drag_frame(&mut self) {
        if !self.editor_drag {
            return;
        }
        let (Some(cam), Some(s), Some(world)) = (
            self.camera.as_ref(),
            self.surface.as_ref(),
            self.world.clone(),
        ) else {
            return;
        };
        let (o, d) = cursor_ray_with_projection(
            cam,
            self.cursor.0,
            self.cursor.1,
            s.config.width as f32,
            s.config.height as f32,
            self.triple_screen_projection_active(),
        );
        let Some(hit) = crate::placing::ground_hit(&world, o, d.as_dvec3(), 400.0) else {
            return;
        };
        let (Some(r), Some(scene), Some(ed)) = (
            self.renderer.as_ref(),
            self.scene.as_mut(),
            self.editor.as_mut(),
        ) else {
            return;
        };
        if let Some(m) = ed.drag_to(&world, r, scene, hit) {
            self.service_msg = Some((m, 3.0));
        }
    }

    pub(crate) fn editor_wheel(&mut self, amount: f32) -> bool {
        if self.editor.is_none() {
            return false;
        }
        let shift =
            self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight);
        let action = if shift {
            crate::editor::Action::Move(glam::DVec3::Z * 0.1 * amount as f64)
        } else {
            crate::editor::Action::Turn(5.0 * amount as f64)
        };
        let (Some(world), Some(r), Some(scene)) = (
            self.world.clone(),
            self.renderer.as_ref(),
            self.scene.as_mut(),
        ) else {
            return true;
        };
        if let Some(m) = self
            .editor
            .as_mut()
            .unwrap()
            .apply(&world, r, scene, &action)
        {
            self.service_msg = Some((m, 3.0));
            self.editor_broadcast(false);
        }
        true
    }
}
