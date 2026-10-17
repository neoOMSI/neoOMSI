use crate::*;

impl Renderer {
    pub(crate) fn install_sky(&mut self, st: atmosphere::SkyState) {
        let mut bytes: Vec<u8> = Vec::with_capacity(st.lut.len() * 8);
        for texel in &st.lut {
            for v in texel {
                bytes.extend_from_slice(&atmosphere::f16_bits(*v).to_le_bytes());
            }
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.sky_lut,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(atmosphere::SKY_LUT_W * 8),
                rows_per_image: Some(atmosphere::SKY_LUT_H),
            },
            wgpu::Extent3d {
                width: atmosphere::SKY_LUT_W,
                height: atmosphere::SKY_LUT_H,
                depth_or_array_layers: 1,
            },
        );
        if ::legacy_config::env::var_os("OMSI_DEBUG_SKY").is_some() {
            log::info!(
                "sky: sun {:?} (altitude {:.1}°) sky {:?} ground {:?} exposure {:.3} table scale {:.4} haze {:.2} overcast {:.2} rain {:.2} sun visibility {:.2}",
                st.sun,
                st.input.sun_dir.z.asin().to_degrees(),
                st.sky_horizontal,
                st.ground,
                st.exposure,
                st.lut_scale,
                st.input.haze,
                st.input.overcast,
                st.input.rain,
                st.input.sun_visibility
            );
        }
        if let Some(p) = self.probe.as_mut() {
            p.age = u32::MAX;
        }
        self.sky_state = Some(st);
    }

    /// How much brighter the enhanced picture is than the plain shading under `lighting`,
    /// for an upward surface at the picture's exposure (1 without enhanced graphics).
    pub fn enhanced_light_ratio(&self, lighting: &Lighting) -> f32 {
        if self.hdr_pass.is_none() || ::legacy_config::env::var_os("OMSI_NO_ENHANCED").is_some() {
            return 1.0;
        }
        let computed;
        let st = match &self.sky_state {
            Some(st) => st,
            None => {
                computed = atmosphere::SkyState::compute(&sky_input(lighting));
                &computed
            }
        };
        let exposure = self.exposure.map(f32::exp).unwrap_or(st.exposure);
        let up = lighting.sun_dir.normalize_or_zero().z.max(0.0);
        let lum = Vec3::new(0.2126, 0.7152, 0.0722);
        let enhanced = (st.sun * up + atmosphere::sh_irradiance(&st.sh, Vec3::Z))
            / std::f32::consts::PI
            * exposure;
        // (as render_inner scales them for a plain frame)
        let plain = lighting.sun_color * lighting.sun_intensity * up
            + (lighting.secondary + lighting.ambient)
                * night_scale(lighting.night, lighting.atmosphere_brightness);
        enhanced.dot(lum) / plain.dot(lum).max(1e-4)
    }

    pub(crate) fn prepare_enhanced(
        &mut self,
        lighting: &Lighting,
        cam_rel: Vec3,
        ro: DVec3,
        dt: f32,
    ) -> bool {
        let input = sky_input(lighting);
        if let Some((_, rx)) = &self.sky_job {
            match rx.try_recv() {
                Ok(st) => {
                    self.sky_job = None;
                    self.install_sky(st);
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.sky_job = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        let pending = self
            .sky_job
            .as_ref()
            .map(|j| j.0)
            .or(self.sky_state.as_ref().map(|st| st.input));
        if pending
            .map(|i| sky_input_differs(&i, &input))
            .unwrap_or(true)
        {
            if self.instant_exposure || self.sky_state.is_none() {
                self.sky_job = None;
                self.install_sky(atmosphere::SkyState::compute(&input));
            } else {
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(atmosphere::SkyState::compute(&input));
                });
                self.sky_job = Some((input, rx));
            }
        }
        let input_overcast = input.overcast;
        let st = self.sky_state.as_ref().expect("sky state");
        let target = st.exposure.max(1e-6).ln();
        let log_exposure = match self.exposure {
            Some(e) if !self.instant_exposure && dt > 0.0 => {
                e + (target - e) * (1.0 - (-dt / 1.5).exp())
            }
            _ => target,
        };
        self.exposure = Some(log_exposure);
        let pre = log_exposure.exp();
        let axes = [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z];
        let avg = axes
            .iter()
            .map(|a| atmosphere::sh_irradiance(&st.sh, *a))
            .fold(Vec3::ZERO, |a, b| a + b)
            / 6.0;
        let fog_rgb = avg * 0.9 / std::f32::consts::PI;
        let weather_fog = if lighting.fog_density > FOG_MIN_DENSITY {
            lighting.fog_density
        } else {
            0.0
        };
        let clear_air = 0.0;
        let base = match lighting.fog_base.or(lighting.inside.map(|v| v.0.z)) {
            Some(z) => (z - ro.z) as f32,
            None => cam_rel.z - 2.0,
        };
        let mut sh = [[0.0f32; 4]; 9];
        for (k, c) in st.sh.iter().enumerate() {
            sh[k] = c.extend(0.0).to_array();
        }
        let instant = self.instant_exposure;
        let (redraw, probe_scale) = match self.probe.as_mut() {
            Some(p) => {
                let redraw = instant || p.age >= 30;
                if redraw {
                    p.age = 0;
                    p.scale = st.lut_scale;
                } else {
                    p.age += 1;
                }
                (redraw, p.scale)
            }
            None => (false, 1.0),
        };
        let cam_w = ro + cam_rel.as_dvec3();
        let eye_off = match self.probe.as_mut() {
            Some(p) => {
                let to_clouds = (1400.0 - cam_rel.z as f64).max(120.0);
                let far = match p.cube_eye {
                    Some(e) if p.cube_filled => {
                        let m = cam_w - e;
                        (m.truncate().length() * 0.3 + m.z.abs()) / to_clouds > 0.0015
                    }
                    _ => true,
                };
                if far {
                    p.cube_eye = Some(cam_w);
                    p.cube_recapture = p.cube_filled;
                }
                (p.cube_eye.unwrap_or(cam_w) - cam_w).as_vec3()
            }
            None => Vec3::ZERO,
        };
        // the weather changed the clouds: draw the sky cube again (not more often than once a
        // second or so)
        if let Some(p) = self.probe.as_mut() {
            let mut sig = [0.0f32; 12];
            for (i, l) in lighting.cloud_layers.iter().enumerate() {
                sig[i * 4..i * 4 + 4].copy_from_slice(&[l[0] / 1000.0, l[1] / 1000.0, l[2], l[3]]);
            }
            p.cloud_age += 1;
            let diff = sig
                .iter()
                .zip(p.cloud_sig.iter())
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max);
            if diff > 0.03 && p.cloud_age >= 60 {
                p.cloud_sig = sig;
                p.cloud_age = 0;
                p.cube_recapture = p.cube_filled;
            }
            let moved = (0..2)
                .map(|i| {
                    let d = lighting.cloud_offset[i] - p.cloud_offset[i];
                    (d - d.round()).abs() * 70000.0
                })
                .fold(0.0f32, f32::max);
            if moved > 6.0 && p.cloud_age >= 8 {
                p.cloud_offset = lighting.cloud_offset;
                p.cloud_age = p.cloud_age.min(8);
                p.cube_recapture = p.cube_filled;
            }
        }
        let u = EnhancedUniform {
            exposure: [
                pre,
                2f32.powf(-self.exposure_log.as_ref().map(|l| l.ev).unwrap_or(0.0))
                    .clamp(0.7, 1.6),
                pre * WINDOW_RADIANCE,
                1.6,
            ],
            sun: st.sun.extend(SUN_RADIUS).to_array(),
            sh,
            ground: st.ground.extend(st.lut_scale).to_array(),
            fog: [weather_fog, 1.0 / 300.0, base, clear_air],
            fog_color: fog_rgb.extend(probe_scale).to_array(),
            weather: [
                lighting.wetness.clamp(0.0, 1.0),
                lighting.snow.clamp(0.0, 1.0),
                lighting.rain.clamp(0.0, 1.0),
                input_overcast,
            ],
            lights: [PROBE_MIPS as f32, LAMP_E, CABIN_E, input.sun_visibility],
            sun_disc: st.sun_disc.extend(dt).to_array(),
            debug: [
                debug_view(),
                ::legacy_config::env::var("OMSI_PUDDLE_F0")
                    .ok()
                    .and_then(|v| v.parse::<f32>().ok())
                    .filter(|v| v.is_finite())
                    .unwrap_or(0.08)
                    .clamp(0.02, 0.2),
                0.0,
                0.0,
            ],
            eye: eye_off.extend(lighting.night.clamp(0.0, 1.0)).to_array(),
            led: [
                lighting.led_glow,
                lighting.led_mips,
                lighting.html_glow,
                lighting.script_glow,
            ],
            layers: lighting.cloud_layers,
        };
        self.write_buffer(&self.enh_buf, 0, bytemuck::bytes_of(&u));
        redraw
    }
}

/// The physical sky's input for `lighting`.
fn sky_input(lighting: &Lighting) -> atmosphere::SkyInput {
    let visibility = 2.3 / lighting.fog_density.max(1e-6);
    let haze = (8000.0 / visibility).clamp(1.0, 6.0) + 2.0 * lighting.rain;
    let wet_cover = (lighting.rain * 1.5).clamp(0.0, 1.0);
    let sun_visibility = lighting.sun_intensity.clamp(0.0, 1.0) * (1.0 - wet_cover);
    atmosphere::SkyInput {
        sun_dir: lighting.sun_dir.normalize_or_zero(),
        sun_visibility,
        overcast: lighting.overcast.clamp(0.0, 1.0).max(wet_cover),
        haze,
        rain: lighting.rain.clamp(0.0, 1.0),
        ground_albedo: 0.2 + 0.45 * lighting.snow.clamp(0.0, 1.0),
        tint: lighting.envir_tint,
        night_light: lighting.atmosphere_brightness,
    }
}
