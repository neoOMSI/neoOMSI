//! Money on the cash desk: coins a passenger pays with (`[ticket_sale_money_point]`) and the
//! change the driver hands out with `GiveChangeCoin` (`[ticket_sale_change_point]`).

use crate::scene::World;
use glam::{DVec3, Mat4, Vec3};
use hashbrown::HashMap;
use ::content::Currency;
use ::geometry::mesh_from_o3d;
use ::render::{AlphaMode, MaterialId, MeshId, Renderer, Scene};
use ::simulation::{VehicleInstance, VehicleType};
use std::path::{Path, PathBuf};

struct Coin {
    inst: usize,
    local: Vec3,
    coin: usize,
    change: bool,
    parent: Option<usize>,
    radius: f32,
    xf: Mat4,
    world: Option<DVec3>,
}

pub struct Money {
    pub currency: Option<Currency>,
    dir: PathBuf,
    meshes: HashMap<usize, (MeshId, Vec<MaterialId>, f32)>,
    placed: Vec<Coin>,
    hidden: Vec<usize>,
    rng: u64,
}

/// The mesh whose `[mesh_ident]` is `name`, the first that carries it.
pub(crate) fn parent_mesh(ty: &VehicleType, name: &str) -> Option<usize> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    ty.meshes.iter().position(|m| {
        ty.model.meshes[m.def_index]
            .mesh_ident
            .as_deref()
            .is_some_and(|i| i.trim().eq_ignore_ascii_case(name))
    })
}

fn coin_transform(bus: &VehicleInstance, rot: Mat4, parent: Option<usize>, local: Vec3) -> Mat4 {
    let base = match parent {
        Some(i) if i < bus.mesh_transforms.len() => bus.mesh_local_transform(i),
        _ => rot,
    };
    base * Mat4::from_translation(local)
}

const TRAY_SLACK: f32 = 0.05;

fn ray_sphere(origin: DVec3, dir: Vec3, spread: f32, center: DVec3, radius: f32) -> Option<f32> {
    let dir = dir.normalize_or_zero();
    let to = (center - origin).as_vec3();
    let t = to.dot(dir);
    if t < 0.0 {
        return None;
    }
    let miss = (to - dir * t).length();
    (miss <= radius + spread * t).then_some(t)
}

impl Money {
    pub fn new(root: &Path, money_system: &str) -> Money {
        let path = ::legacy_config::resolve_path(root, money_system);
        let currency = Currency::load(&path)
            .map_err(|e| log::warn!("money system {}: {e}", path.display()))
            .ok();
        if let Some(c) = &currency {
            log::info!(
                "money system {}: {} coins, {} bills",
                c.name,
                c.coins.len(),
                c.bills.len()
            );
        }
        Money {
            currency,
            dir: path.parent().map(|p| p.to_path_buf()).unwrap_or_default(),
            meshes: HashMap::new(),
            placed: Vec::new(),
            hidden: Vec::new(),
            rng: 0x5151_7777,
        }
    }

    fn rand_f(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Every coin and note of the currency as (index, value): the coins first, then the
    /// notes (index `coins.len() + k`). The passengers had only ever paid in coins - a
    /// Novi Sad fare of 65 dinars was always 20 + 20 + 20 + 5, never a 100 note.
    fn denominations(&self) -> Vec<(usize, f32)> {
        let Some(c) = &self.currency else {
            return Vec::new();
        };
        let n = c.coins.len();
        let mut all: Vec<(usize, f32)> = c
            .coins
            .iter()
            .enumerate()
            .map(|(i, (_, v))| (i, *v))
            .chain(c.bills.iter().enumerate().map(|(k, (_, v))| (n + k, *v)))
            .filter(|(_, v)| *v > 0.0)
            .collect();
        all.sort_by(|a, b| b.1.total_cmp(&a.1));
        all
    }

    /// `value` in as few pieces as the currency allows, largest first; None when it cannot
    /// be paid exactly with them (a fraction smaller than the smallest coin).
    fn exact_pieces(all: &[(usize, f32)], value: f32) -> Option<Vec<usize>> {
        let mut out = Vec::new();
        let mut left = value;
        for (i, v) in all {
            while left >= *v - 0.001 && out.len() < 40 {
                out.push(*i);
                left -= v;
            }
        }
        (left.abs() <= 0.001).then_some(out)
    }

    /// The fare exactly (no change due): coins and notes. (Capped at twelve coins and topped
    /// up with the smallest when that ran out, "exact" was not always exact.)
    pub fn exact_coins_for(&mut self, value: f32) -> Vec<usize> {
        let all = self.denominations();
        if let Some(out) = Self::exact_pieces(&all, value) {
            return out;
        }
        // not payable to the last cent: the nearest amount over it
        let mut out = Vec::new();
        let mut left = value;
        for (i, v) in &all {
            while left >= *v - 0.001 && out.len() < 40 {
                out.push(*i);
                left -= v;
            }
        }
        if left > 0.001 {
            if let Some((i, _)) = all.iter().rev().find(|(_, v)| *v >= left) {
                out.push(*i);
            }
        }
        out
    }

    /// What a passenger puts on the desk for `price`, as Omsi.exe does it (sub_7e8254):
    /// coins drawn at random until they cover the price, then every coin that is not needed
    /// (the rest still covers the price less half the smallest coin) taken back again.
    pub fn omsi_coins_for(&mut self, price: f32) -> Vec<usize> {
        let values: Vec<f32> = match &self.currency {
            Some(c) => c.coins.iter().map(|(_, v)| *v).collect(),
            None => return Vec::new(),
        };
        if values.is_empty() || values.iter().all(|v| *v <= 0.0) {
            return self.exact_coins_for(price);
        }
        let half_smallest = self.smallest_value() / 2.0;
        let mut out: Vec<usize> = Vec::new();
        let mut sum = 0.0f32;
        while sum < price && out.len() < 200 {
            let k = ((self.rand_f() * values.len() as f32) as usize).min(values.len() - 1);
            sum += values[k];
            out.push(k);
        }
        let mut again = true;
        while again {
            again = false;
            for j in 0..out.len() {
                if price - half_smallest <= sum - values[out[j]] {
                    sum -= values[out[j]];
                    out.remove(j);
                    again = true;
                    break;
                }
            }
        }
        out
    }

    /// The value of the smallest coin (the tolerance of the change is half of it).
    pub fn smallest_value(&self) -> f32 {
        self.currency
            .as_ref()
            .and_then(|c| {
                c.coins
                    .iter()
                    .map(|(_, v)| *v)
                    .filter(|v| *v > 0.0)
                    .reduce(f32::min)
            })
            .unwrap_or(0.01)
    }

    /// How many coins lie on the change tray.
    pub fn change_count(&self) -> usize {
        self.placed.iter().filter(|p| p.change).count()
    }

    pub fn value_of(&self, coins: &[usize]) -> f32 {
        let Some(c) = &self.currency else { return 0.0 };
        let n = c.coins.len();
        coins
            .iter()
            .filter_map(|&i| {
                if i < n {
                    c.coins.get(i)
                } else {
                    c.bills.get(i - n)
                }
            })
            .map(|(_, v)| *v)
            .sum()
    }

    fn mesh(
        &mut self,
        world: &World,
        renderer: &Renderer,
        scene: &mut Scene,
        coin: usize,
    ) -> Option<(MeshId, Vec<MaterialId>, f32)> {
        if let Some(m) = self.meshes.get(&coin) {
            return Some(m.clone());
        }
        let c = self.currency.as_ref()?;
        let n = c.coins.len();
        let file = if coin < n {
            c.coins.get(coin)?.0.clone()
        } else {
            c.bills.get(coin - n)?.0.clone()
        };
        let m = ::legacy_o3d::load_mesh(&::legacy_config::resolve_path(&self.dir, &file))
            .map_err(|e| log::warn!("{e}"))
            .ok()?;
        let dirs = [
            self.dir.clone(),
            ::legacy_config::resolve_path(&world.root, "Texture"),
        ];
        let dirs_ref: Vec<&Path> = dirs.iter().map(|p| p.as_path()).collect();
        let mats: Vec<MaterialId> = m
            .materials
            .iter()
            .map(|mat| {
                let tex = ::texture::find_texture(&mat.texture, &dirs_ref)
                    .and_then(|p| world.textures.get_gpu_fast(&p))
                    .map(|(img, _)| renderer.add_texture_data(scene, &img));
                renderer.add_material(scene, tex, AlphaMode::Opaque, [1.0; 4], false)
            })
            .collect();
        let radius = m
            .vertices
            .iter()
            .map(|v| v.position.length())
            .fold(0.0, f32::max);
        let id = renderer.add_mesh(scene, &mesh_from_o3d(&m));
        self.meshes.insert(coin, (id, mats.clone(), radius));
        Some((id, mats, radius))
    }

    /// Put coins on a point of the cabin (position + variation), stacked, moving with the
    /// `parent` mesh when there is one.
    #[allow(clippy::too_many_arguments)]
    pub fn place(
        &mut self,
        world: &World,
        renderer: &Renderer,
        scene: &mut Scene,
        coins: &[usize],
        point: Vec3,
        var: [f32; 2],
        parent: Option<usize>,
        change: bool,
    ) {
        let count = self.placed.iter().filter(|p| p.change == change).count();
        for (k, coin) in coins.iter().enumerate() {
            let Some((id, mats, radius)) = self.mesh(world, renderer, scene, *coin) else {
                continue;
            };
            let local = point
                + Vec3::new(
                    (self.rand_f() - 0.5) * var[0],
                    (self.rand_f() - 0.5) * var[1],
                    0.003 * (count + k) as f32,
                );
            let inst = renderer.add_instance(scene, id, DVec3::ZERO, Mat4::IDENTITY, mats);
            self.placed.push(Coin {
                inst,
                local,
                coin: *coin,
                change,
                parent,
                radius,
                xf: Mat4::IDENTITY,
                world: None,
            });
        }
    }

    /// Remove the payment (driver takes it) or the change (passenger takes it).
    pub fn clear(&mut self, change: bool) {
        let (gone, keep): (Vec<_>, Vec<_>) =
            self.placed.drain(..).partition(|p| p.change == change);
        self.hidden.extend(gone.into_iter().map(|p| p.inst));
        self.placed = keep;
    }

    pub fn change_under(
        &self,
        origin: DVec3,
        dir: Vec3,
        spread: f32,
        wall: impl FnOnce() -> Option<f32>,
    ) -> Option<usize> {
        let (k, front) = self
            .placed
            .iter()
            .enumerate()
            .filter(|(_, c)| c.change)
            .filter_map(|(k, c)| {
                Some((k, ray_sphere(origin, dir, spread, c.world?, c.radius)? - c.radius))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))?;
        (!wall().is_some_and(|w| w < front - TRAY_SLACK)).then_some(k)
    }

    pub fn pick(
        &mut self,
        origin: DVec3,
        dir: Vec3,
        spread: f32,
        wall: impl FnOnce() -> Option<f32>,
    ) -> bool {
        let Some(k) = self.change_under(origin, dir, spread, wall) else {
            return false;
        };
        let gone = self.placed.remove(k);
        self.hidden.push(gone.inst);
        true
    }

    pub fn change_value(&self) -> f32 {
        let coins: Vec<usize> = self
            .placed
            .iter()
            .filter(|p| p.change)
            .map(|p| p.coin)
            .collect();
        self.value_of(&coins)
    }

    fn locate(&mut self, bus: &VehicleInstance) {
        let rot = bus.body_rotation();
        for c in &mut self.placed {
            c.xf = coin_transform(bus, rot, c.parent, c.local);
            c.world = Some(bus.position + c.xf.w_axis.truncate().as_dvec3());
        }
    }

    pub fn sync(&mut self, renderer: &Renderer, scene: &mut Scene, bus: &VehicleInstance) {
        for inst in self.hidden.drain(..) {
            renderer.set_params(scene, inst, &[], false, &[]);
        }
        self.locate(bus);
        for c in &self.placed {
            renderer.set_transform(scene, c.inst, bus.position, c.xf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;
    use std::f32::consts::FRAC_PI_2;
    use std::sync::Arc;

    fn money() -> Money {
        Money {
            currency: Some(Currency {
                path: PathBuf::new(),
                name: "test".into(),
                decimals: 2,
                coins: vec![("small.o3d".into(), 0.5), ("big.o3d".into(), 2.0)],
                bills: Vec::new(),
            }),
            dir: PathBuf::new(),
            meshes: HashMap::new(),
            placed: Vec::new(),
            hidden: Vec::new(),
            rng: 1,
        }
    }

    fn put(
        m: &mut Money,
        inst: usize,
        coin: usize,
        change: bool,
        local: Vec3,
        parent: Option<usize>,
    ) {
        m.placed.push(Coin {
            inst,
            local,
            coin,
            change,
            parent,
            radius: 0.012,
            xf: Mat4::IDENTITY,
            world: None,
        });
    }

    fn put_at(m: &mut Money, inst: usize, coin: usize, change: bool, world: DVec3) {
        put(m, inst, coin, change, Vec3::ZERO, None);
        m.placed.last_mut().unwrap().world = Some(world);
    }

    fn o3d_triangle() -> Vec<u8> {
        let mut bytes = vec![0x84, 0x19, 1, 0x17, 3, 0];
        for x in [0.0f32, 0.1, 0.2] {
            for value in [x, 0.0, 0.1, 0.0, 1.0, 0.0, 0.0, 0.0] {
                bytes.extend(value.to_le_bytes());
            }
        }
        bytes.extend([0x49, 1, 0, 0, 0, 1, 0, 2, 0, 0, 0]);
        bytes
    }

    #[test]
    fn coins_on_a_parented_point_move_with_its_mesh() {
        let dir = std::env::temp_dir().join(format!("omsi-money-parent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("test.bus"), "[model]\nmodel.cfg\n").unwrap();
        std::fs::write(
            dir.join("model.cfg"),
            "[mesh]\nbody.o3d\n\n[mesh]\ndesk.o3d\n[mesh_ident]\nzahltisch\n",
        )
        .unwrap();
        std::fs::write(dir.join("body.o3d"), o3d_triangle()).unwrap();
        std::fs::write(dir.join("desk.o3d"), o3d_triangle()).unwrap();
        let ty = Arc::new(VehicleType::load(&dir, &dir.join("test.bus")).unwrap());
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(parent_mesh(&ty, " Zahltisch "), Some(1));
        assert_eq!(parent_mesh(&ty, "kasse"), None);

        let mut bus = VehicleInstance::new(ty, ::simulation::VehicleHost::new(Default::default()));
        bus.position = DVec3::new(100.0, 200.0, 5.0);
        let pivot = Vec3::new(-0.5, 4.8, 1.0);
        bus.mesh_transforms[1] = Mat4::from_translation(pivot)
            * Mat4::from_rotation_z(FRAC_PI_2)
            * Mat4::from_translation(-pivot);
        let local = Vec3::new(-0.1, 4.8, 1.2);
        let mut m = money();
        put(&mut m, 0, 0, true, local, Some(1));
        put(&mut m, 1, 0, true, local, None);
        put(&mut m, 2, 0, false, local, Some(99));
        m.locate(&bus);

        let rot = bus.body_rotation();
        let at = |p: Vec3| bus.position + rot.transform_point3(p).as_dvec3();
        let swung = pivot + Quat::from_rotation_z(FRAC_PI_2) * (local - pivot);
        let near = |a: DVec3, b: DVec3| (a - b).length() < 1e-4;
        assert!(
            near(m.placed[0].world.unwrap(), at(swung)),
            "{:?}",
            m.placed[0].world
        );
        assert!(near(m.placed[1].world.unwrap(), at(local)));
        assert!(near(m.placed[2].world.unwrap(), at(local)));
    }

    #[test]
    fn pick_takes_the_nearest_change_coin_the_ray_hits() {
        let mut m = money();
        put_at(&mut m, 10, 0, false, DVec3::new(0.0, 0.5, 0.0));
        put_at(&mut m, 11, 0, true, DVec3::new(0.0, 2.0, 0.0));
        put_at(&mut m, 12, 1, true, DVec3::new(0.005, 1.0, 0.0));
        put(&mut m, 13, 0, true, Vec3::ZERO, None);

        assert!(!m.pick(DVec3::ZERO, Vec3::X, 0.0, || None));
        assert!(!m.pick(DVec3::ZERO, -Vec3::Y, 0.0, || None));
        assert!(!m.pick(DVec3::new(0.0, 0.0, 0.1), Vec3::Y, 0.0, || None));
        assert_eq!(m.change_count(), 3);

        assert!(m.pick(DVec3::ZERO, Vec3::Y, 0.0, || None));
        assert_eq!(m.hidden, vec![12]);
        assert!(m.pick(DVec3::ZERO, Vec3::Y, 0.0, || None));
        assert_eq!(m.hidden, vec![12, 11]);
        assert!(
            !m.pick(DVec3::ZERO, Vec3::Y, 0.0, || None),
            "the payment and unplaced coins stay"
        );
        assert_eq!(
            m.placed.iter().map(|c| c.inst).collect::<Vec<_>>(),
            vec![10, 13]
        );

        let mut wide = money();
        put_at(&mut wide, 0, 0, true, DVec3::new(0.05, 1.0, 0.0));
        assert!(!wide.pick(DVec3::ZERO, Vec3::Y, 0.0, || None));
        assert!(wide.pick(DVec3::ZERO, Vec3::Y, 0.05, || None));
    }

    #[test]
    fn a_coin_behind_the_bus_cannot_be_picked() {
        let mut m = money();
        put_at(&mut m, 0, 0, true, DVec3::new(0.0, 1.0, 0.0));
        assert!(!m.pick(DVec3::ZERO, Vec3::Y, 0.0, || Some(0.5)));
        assert_eq!(m.change_under(DVec3::ZERO, Vec3::Y, 0.0, || Some(0.5)), None);
        assert_eq!(m.change_count(), 1);
        assert!(
            m.pick(DVec3::ZERO, Vec3::Y, 0.0, || Some(0.97)),
            "the tray right under the coin does not hide it"
        );

        let mut far = money();
        put_at(&mut far, 0, 0, true, DVec3::new(0.0, 1.0, 0.0));
        let mut asked = false;
        assert!(far
            .change_under(DVec3::ZERO, Vec3::X, 0.0, || {
                asked = true;
                Some(0.1)
            })
            .is_none());
        assert!(!asked, "no coin under the ray: the bus is not tested");
    }

    #[test]
    fn picking_every_change_coin_leaves_what_change_take_leaves() {
        let tray = || {
            let mut m = money();
            put_at(&mut m, 0, 1, false, DVec3::new(0.0, 0.5, 0.0));
            put_at(&mut m, 1, 0, true, DVec3::new(0.0, 1.0, 0.0));
            put_at(&mut m, 2, 1, true, DVec3::new(0.0, 2.0, 0.0));
            m
        };
        let mut picked = tray();
        assert!((picked.change_value() - 2.5).abs() < 1e-6);
        assert!(picked.pick(DVec3::ZERO, Vec3::Y, 0.0, || None));
        assert!((picked.change_value() - 2.0).abs() < 1e-6);
        assert_eq!(picked.change_count(), 1);
        assert!(picked.pick(DVec3::ZERO, Vec3::Y, 0.0, || None));

        let mut taken = tray();
        taken.clear(true);
        assert_eq!(picked.change_value(), taken.change_value());
        assert_eq!(picked.change_count(), taken.change_count());
        let mut a = picked.hidden.clone();
        a.sort();
        assert_eq!(a, taken.hidden);
        assert_eq!(
            picked
                .placed
                .iter()
                .map(|c| (c.inst, c.coin, c.change))
                .collect::<Vec<_>>(),
            taken
                .placed
                .iter()
                .map(|c| (c.inst, c.coin, c.change))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn stock_cash_desk_points_resolve_their_parent_mesh() {
        let Some(root) = ::legacy_config::env::var_os("OMSI_ROOT").map(PathBuf::from) else {
            eprintln!("skipped: OMSI_ROOT not set");
            return;
        };
        let bus = root.join("Vehicles/MAN_NL_NG/MAN_EN92_main.bus");
        if !bus.exists() {
            eprintln!("skipped: no {}", bus.display());
            return;
        }
        let ty = VehicleType::load(&root, &bus).expect("EN92");
        let rel = ty.def.passenger_cabin.as_ref().expect("passenger cabin");
        let cabin = ::legacy_vehicle::PassengerCabin::load(&::legacy_config::resolve_path(
            ty.def.dir(),
            rel,
        ))
        .expect("cabin");
        let parents: Vec<&str> = cabin
            .money_points
            .iter()
            .chain(&cabin.change_points)
            .filter_map(|p| p.parent.as_deref())
            .collect();
        assert!(!parents.is_empty());
        for p in parents {
            assert!(parent_mesh(&ty, p).is_some(), "no mesh {p}");
        }
    }
}
