use super::*;

pub(crate) fn trigger_leaves(program: &::legacy_script::Program, name: &str) -> Vec<String> {
    fn walk(
        program: &::legacy_script::Program,
        block: ::legacy_script::BlockId,
        seen: &mut hashbrown::HashSet<::legacy_script::BlockId>,
        stored: &mut Vec<String>,
        read: &mut Vec<String>,
    ) {
        if !seen.insert(block) || seen.len() > 64 {
            return;
        }
        let Some(b) = program.blocks.get(block as usize) else {
            return;
        };
        for op in &b.ops {
            match op {
                ::legacy_script::Op::Store(id) | ::legacy_script::Op::Load(id) => {
                    let Some(leaf) = program
                        .var_names
                        .get(*id as usize)
                        .and_then(|n| door_leaf_of(n))
                    else {
                        continue;
                    };
                    let list = if matches!(op, ::legacy_script::Op::Store(_)) {
                        &mut *stored
                    } else {
                        &mut *read
                    };
                    if !list.contains(&leaf) {
                        list.push(leaf);
                    }
                }
                ::legacy_script::Op::Macro(m) => walk(program, *m, seen, stored, read),
                _ => {}
            }
        }
    }
    let Some(b) = program.trigger(name) else {
        return Vec::new();
    };
    let (mut stored, mut read) = (Vec::new(), Vec::new());
    walk(
        program,
        b,
        &mut hashbrown::HashSet::new(),
        &mut stored,
        &mut read,
    );
    if stored.is_empty() { read } else { stored }
}

pub(super) fn doorways(ty: &::simulation::VehicleType) -> Option<Vec<Vec<String>>> {
    let mut at: Vec<(String, f32, u32)> = Vec::new();
    for vm in &ty.meshes {
        let Some(def) = ty.model.meshes.get(vm.def_index) else {
            continue;
        };
        let Some(leaf) = def
            .animations
            .iter()
            .find_map(|a| door_leaf_of(&a.variable))
        else {
            continue;
        };
        if vm.data.positions.is_empty() {
            continue;
        }
        let y = vm
            .data
            .positions
            .iter()
            .map(|p| vm.pivot.transform_point3(*p).y)
            .sum::<f32>()
            / vm.data.positions.len() as f32;
        match at.iter_mut().find(|e| e.0 == leaf) {
            Some(e) => {
                e.1 += y;
                e.2 += 1;
            }
            None => at.push((leaf, y, 1)),
        }
    }
    if at.is_empty() {
        return None;
    }
    let mut leaves: Vec<(String, f32)> =
        at.into_iter().map(|(l, y, n)| (l, y / n as f32)).collect();
    leaves.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut ways: Vec<Vec<(String, f32)>> = Vec::new();
    for l in leaves {
        match ways.last_mut() {
            Some(w) if (w.last().unwrap().1 - l.1).abs() < 1.6 => w.push(l),
            _ => ways.push(vec![l]),
        }
    }
    let program = &ty.program;
    let way_of = |leaf: &String| ways.iter().position(|w| w.iter().any(|(l, _)| l == leaf));
    let reach = |name: &str| -> Vec<usize> {
        let mut v: Vec<usize> = trigger_leaves(program, name)
            .iter()
            .filter_map(way_of)
            .collect();
        v.sort();
        v.dedup();
        v
    };
    let mut groups: Vec<(Vec<String>, usize)> = Vec::new();
    for g in door_trigger_groups(program) {
        if g == ["bus_dooraft"] {
            continue;
        }
        let parts: Vec<Vec<String>> = match (g.first(), g.get(1)) {
            (Some(a), Some(b))
            if !reach(a).is_empty()
                && !reach(b).is_empty()
                && !reach(a).iter().any(|w| reach(b).contains(w)) =>
                {
                    vec![vec![a.clone()], vec![b.clone()]]
                }
            _ => vec![g.clone()],
        };
        for part in parts {
            let front = part
                .iter()
                .flat_map(|n| reach(n))
                .min()
                .unwrap_or(usize::MAX);
            groups.push((part, front));
        }
    }
    let reached: Vec<usize> = groups
        .iter()
        .flat_map(|(g, _)| g.iter().flat_map(|n| reach(n)))
        .collect();
    let mut names: Vec<&String> = program
        .triggers
        .keys()
        .filter(|n| {
            let n = n.to_ascii_lowercase();
            (n.starts_with("bus_door") || n.starts_with("bus_tuer"))
                && !n.ends_with("_off")
                && !n.ends_with("_close")
                && !n.starts_with("bus_doorfront")
                && n != "bus_dooraft"
        })
        .collect();
    names.sort();
    for w in 0..ways.len() {
        if reached.contains(&w) {
            continue;
        }
        let mut group: Vec<String> = Vec::new();
        for n in &names {
            let r = reach(n);
            if r != [w] {
                continue;
            }
            let close = format!("{n}_close");
            group.push(if program.trigger(&close).is_some() {
                format!("{n}|{close}")
            } else {
                (*n).clone()
            });
        }
        if !group.is_empty() {
            groups.push((group, w));
        }
    }
    groups.sort_by_key(|(_, front)| *front);
    let mut out: Vec<Vec<String>> = groups.into_iter().map(|(g, _)| g).collect();
    if program.trigger("bus_dooraft").is_some() {
        out.push(vec!["bus_dooraft".to_string()]);
    }
    (!out.is_empty()).then_some(out)
}

pub(crate) fn door_keys(ty: &::simulation::VehicleType) -> Vec<Vec<String>> {
    doorways(ty).unwrap_or_else(|| door_trigger_groups(&ty.program))
}

pub(crate) fn door_trigger_groups(program: &::legacy_script::Program) -> Vec<Vec<String>> {
    let mut fronts: Vec<(u32, String)> = program
        .triggers
        .keys()
        .filter_map(|n| {
            n.strip_prefix("bus_doorfront")
                .and_then(|rest| rest.parse::<u32>().ok())
                .map(|i| (i, n.clone()))
        })
        .collect();
    fronts.sort_by_key(|(i, _)| *i);
    let mut out = Vec::new();
    let mut i = 0;
    while i < fronts.len() {
        let mut group = vec![fronts[i].1.clone()];
        if let Some((next, name)) = fronts.get(i + 1) {
            if *next == fronts[i].0 + 1 && !door_trigger_closes(program, name) {
                group.push(name.clone());
                i += 1;
            }
        }
        out.push(group);
        i += 1;
    }
    if program.trigger("bus_dooraft").is_some() {
        out.push(vec!["bus_dooraft".to_string()]);
    }
    out
}

pub(crate) fn door_trigger_closes(program: &::legacy_script::Program, name: &str) -> bool {
    let Some(b) = program
        .trigger(name)
        .and_then(|b| program.blocks.get(b as usize))
    else {
        return false;
    };
    if b.ops.len() > 6
        || b.ops
        .iter()
        .any(|op| matches!(op, ::legacy_script::Op::Macro(_)))
    {
        return false;
    }
    b.ops.iter().any(|op| match op {
        ::legacy_script::Op::Store(id) => {
            let n = program
                .var_names
                .get(*id as usize)
                .map(|n| n.to_ascii_lowercase())
                .unwrap_or_default();
            ["close", "closing", "schliess", "schließ"]
                .iter()
                .any(|x| n.contains(x))
        }
        _ => false,
    })
}

pub(crate) fn door_trigger_target(
    program: &::legacy_script::Program,
    name: &str,
) -> Option<::legacy_script::VarId> {
    fn find(
        program: &::legacy_script::Program,
        block: ::legacy_script::BlockId,
        seen: &mut hashbrown::HashSet<::legacy_script::BlockId>,
    ) -> Option<::legacy_script::VarId> {
        if !seen.insert(block) {
            return None;
        }
        let b = program.blocks.get(block as usize)?;
        for op in &b.ops {
            match op {
                ::legacy_script::Op::Store(id) => {
                    let n = program
                        .var_names
                        .get(*id as usize)
                        .map(|n| n.to_ascii_lowercase())
                        .unwrap_or_default();
                    if n.contains("target") || n.contains("soll") {
                        return Some(*id);
                    }
                }
                ::legacy_script::Op::Macro(m) => {
                    if let Some(v) = find(program, *m, seen) {
                        return Some(v);
                    }
                }
                _ => {}
            }
        }
        None
    }
    program
        .trigger(name)
        .and_then(|b| find(program, b, &mut hashbrown::HashSet::new()))
}

pub(crate) fn door_group_to_fire(
    v: &mut ::simulation::VehicleInstance,
    group: &[String],
) -> Vec<String> {
    let fire = door_group_plan(v, group);
    if fire.len() < 2 {
        return fire;
    }
    let mut targets: Vec<::legacy_script::VarId> = group
        .iter()
        .filter_map(|n| door_trigger_target(&v.ty.program, n.split('|').next().unwrap_or(n)))
        .collect();
    targets.sort_unstable();
    targets.dedup();
    if targets.is_empty() {
        return fire;
    }
    let at = |vars: &[f32]| -> Vec<bool> {
        targets
            .iter()
            .map(|&t| vars.get(t as usize).is_some_and(|x| *x > 0.5))
            .collect()
    };
    let base = at(&v.state.vars);
    let names: Vec<&str> = fire.iter().map(|s| s.as_str()).collect();
    if at(&v.trial_triggers(&names)) != base {
        return fire;
    }
    match fire
        .iter()
        .find(|n| at(&v.trial_triggers(&[n.as_str()])) != base)
    {
        Some(one) => vec![one.clone()],
        None => fire,
    }
}

pub(super) fn door_group_plan(v: &::simulation::VehicleInstance, group: &[String]) -> Vec<String> {
    let group: Vec<String> = group
        .iter()
        .map(|n| match n.split_once('|') {
            Some((open, close)) => {
                let leaf = trigger_leaves(&v.ty.program, open);
                let is_open = leaf.iter().any(|l| v.var(l).is_some_and(|x| x > 0.5));
                if is_open {
                    close.to_string()
                } else {
                    open.to_string()
                }
            }
            None => n.clone(),
        })
        .collect();
    let group = &group[..];
    let states: Vec<Option<bool>> = group
        .iter()
        .map(|n| {
            door_trigger_target(&v.ty.program, n)
                .and_then(|id| v.state.vars.get(id as usize).copied())
                .map(|x| x > 0.5)
        })
        .collect();
    if group.len() < 2 || states.iter().any(|s| s.is_none()) {
        return group.to_vec();
    }
    let any_open = states.iter().any(|s| *s == Some(true));
    group
        .iter()
        .zip(&states)
        .filter(|(_, s)| !any_open || **s == Some(true))
        .map(|(n, _)| n.clone())
        .collect()
}
