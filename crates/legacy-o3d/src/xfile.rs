//! Minimal DirectX `.x` mesh reader: enough for the OMSI helper meshes and the few
//! scenery objects that still ship `.x` files (frames, Mesh, MeshNormals, MeshTextureCoords,
//! the UVs in DeclData, MeshMaterialList, Material, TextureFilename). Binary files are read
//! as the text they encode; compressed ones are not used by OMSI content.

use super::{Material, Mesh, O3dError, Triangle, Vertex};
use glam::{Mat3, Mat4, Vec2, Vec3};
use std::collections::HashMap;

struct Tok<'a> {
    s: &'a str,
    p: usize,
}

impl<'a> Tok<'a> {
    fn skip_ws(&mut self) {
        let b = self.s.as_bytes();
        loop {
            while self.p < b.len() && (b[self.p] as char).is_whitespace() {
                self.p += 1;
            }
            if self.p + 1 < b.len() && b[self.p] == b'/' && b[self.p + 1] == b'/' {
                while self.p < b.len() && b[self.p] != b'\n' {
                    self.p += 1;
                }
                continue;
            }
            if self.p < b.len() && b[self.p] == b'#' {
                while self.p < b.len() && b[self.p] != b'\n' {
                    self.p += 1;
                }
                continue;
            }
            break;
        }
    }
    fn next(&mut self) -> Option<&'a str> {
        self.skip_ws();
        let b = self.s.as_bytes();
        if self.p >= b.len() {
            return None;
        }
        let start = self.p;
        let c = b[self.p];
        if c == b'"' {
            self.p += 1;
            while self.p < b.len() && b[self.p] != b'"' {
                self.p += 1;
            }
            self.p += 1;
            return Some(&self.s[start..self.p.min(b.len())]);
        }
        if b"{};,;<>".contains(&c) {
            self.p += 1;
            return Some(&self.s[start..self.p]);
        }
        while self.p < b.len()
            && !(b[self.p] as char).is_whitespace()
            && !b"{};,<>\"".contains(&b[self.p])
        {
            self.p += 1;
        }
        Some(&self.s[start..self.p])
    }
    fn peek(&mut self) -> Option<&'a str> {
        let save = self.p;
        let t = self.next();
        self.p = save;
        t
    }
    fn expect(&mut self, t: &str) -> Result<(), O3dError> {
        match self.next() {
            Some(x) if x == t => Ok(()),
            other => Err(O3dError::XFile(format!("expected '{t}', got {other:?}"))),
        }
    }
    fn number(&mut self) -> Result<f32, O3dError> {
        loop {
            let t = self
                .next()
                .ok_or_else(|| O3dError::XFile("eof in number".into()))?;
            if t == "," || t == ";" {
                continue;
            }
            return t
                .parse::<f32>()
                .map_err(|_| O3dError::XFile(format!("bad number {t}")));
        }
    }
    fn int(&mut self) -> Result<usize, O3dError> {
        Ok(self.number()? as usize)
    }
    /// A DWORD as written, without the detour through `f32` (`DeclData` stores float bits).
    fn dword(&mut self) -> Result<u32, O3dError> {
        loop {
            let t = self
                .next()
                .ok_or_else(|| O3dError::XFile("eof in dword".into()))?;
            if t == "," || t == ";" {
                continue;
            }
            return t
                .parse::<u32>()
                .map_err(|_| O3dError::XFile(format!("bad dword {t}")));
        }
    }
    /// Skip a `{ ... }` block (after the '{' has been consumed).
    fn skip_block(&mut self) -> Result<(), O3dError> {
        let mut depth = 1;
        while depth > 0 {
            match self.next() {
                Some("{") => depth += 1,
                Some("}") => depth -= 1,
                Some(_) => {}
                None => return Err(O3dError::XFile("eof in block".into())),
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct Ctx {
    mesh: Mesh,
    /// Named data objects in an .x file can be referenced from any frame or mesh.
    named_materials: HashMap<String, Material>,
    /// Resolve after the whole file, because definitions may follow their references.
    material_refs: Vec<(usize, String)>,
}

pub fn parse_x(bytes: &[u8]) -> Result<Mesh, O3dError> {
    // (the 16-byte header is checked as bytes: a short file or a non-ASCII byte there made
    // the text slices below panic)
    if bytes.len() < 16 || !bytes[..16].is_ascii() || !bytes.starts_with(b"xof ") {
        return Err(O3dError::XFile("missing xof header".into()));
    }
    let binary;
    let bytes = match &bytes[8..12] {
        b"txt " => bytes,
        b"bin " => {
            binary = binary_to_text(bytes)?;
            &binary[..]
        }
        f => {
            return Err(O3dError::XFile(format!(
                "unsupported .x format {:?}",
                String::from_utf8_lossy(f)
            )));
        }
    };
    let (text, _) = encoding_rs::WINDOWS_1252.decode_without_bom_handling(bytes);
    let text = text.into_owned();
    let mut t = Tok { s: &text, p: 16 };
    let mut ctx = Ctx {
        mesh: Mesh {
            transform: Mat4::IDENTITY,
            ..Default::default()
        },
        ..Default::default()
    };
    parse_objects(&mut t, &mut ctx, Mat4::IDENTITY)?;
    for (slot, name) in &ctx.material_refs {
        if let Some(material) = ctx.named_materials.get(name) {
            ctx.mesh.materials[*slot] = material.clone();
        }
    }
    if ctx.mesh.materials.is_empty() {
        ctx.mesh.materials.push(Material::default());
    }
    ctx.mesh.drop_bad_triangles();
    Ok(ctx.mesh)
}

/// A binary `.x` file (`xof 0303bin`) as the text it encodes, for the text reader. Names keep
/// only letters, digits, `_` and `-`: a binary name may hold a space, which would split it.
fn binary_to_text(bytes: &[u8]) -> Result<Vec<u8>, O3dError> {
    let double = &bytes[12..16] == b"0064";
    let mut out = b"xof 0303txt 0032\n".to_vec();
    let mut p = 16;
    let eof = || O3dError::XFile("eof in binary token".into());
    let take = |p: &mut usize, n: usize| -> Result<&[u8], O3dError> {
        let s = bytes.get(*p..*p + n).ok_or_else(eof)?;
        *p += n;
        Ok(s)
    };
    let dword = |p: &mut usize| -> Result<u32, O3dError> {
        Ok(u32::from_le_bytes(take(p, 4)?.try_into().unwrap()))
    };
    while p < bytes.len() {
        let token = u16::from_le_bytes(take(&mut p, 2)?.try_into().unwrap());
        match token {
            // TOKEN_NAME
            1 => {
                let n = dword(&mut p)? as usize;
                for &c in take(&mut p, n)? {
                    out.push(if c.is_ascii_alphanumeric() || c == b'-' {
                        c
                    } else {
                        b'_'
                    });
                }
                out.push(b' ');
            }
            // TOKEN_STRING, with its terminator token
            2 => {
                let n = dword(&mut p)? as usize;
                let s = take(&mut p, n)?;
                out.push(b'"');
                out.extend(s.iter().filter(|&&c| c != b'"' && c != 0));
                out.push(b'"');
                take(&mut p, 2)?;
                out.extend_from_slice(b";\n");
            }
            // TOKEN_INTEGER
            3 => out.extend_from_slice(format!("{};\n", dword(&mut p)?).as_bytes()),
            // TOKEN_GUID
            5 => {
                let g = take(&mut p, 16)?;
                out.push(b'<');
                for b in g {
                    out.extend_from_slice(format!("{b:02X}").as_bytes());
                }
                out.extend_from_slice(b">\n");
            }
            // TOKEN_INTEGER_LIST, TOKEN_FLOAT_LIST
            6 | 7 => {
                let n = dword(&mut p)? as usize;
                for _ in 0..n {
                    let v = if token == 6 {
                        dword(&mut p)?.to_string()
                    } else if double {
                        f64::from_le_bytes(take(&mut p, 8)?.try_into().unwrap()).to_string()
                    } else {
                        f32::from_le_bytes(take(&mut p, 4)?.try_into().unwrap()).to_string()
                    };
                    out.extend_from_slice(v.as_bytes());
                    out.push(b',');
                }
                out.extend_from_slice(b";\n");
            }
            10 => out.extend_from_slice(b"{\n"),
            11 => out.extend_from_slice(b"}\n"),
            12 => out.push(b'('),
            13 => out.push(b')'),
            14 => out.push(b'['),
            15 => out.push(b']'),
            16 => out.push(b'<'),
            17 => out.push(b'>'),
            18 => out.push(b'.'),
            19 => out.push(b','),
            20 => out.extend_from_slice(b";\n"),
            31 => out.extend_from_slice(b"template "),
            // primitive type words and `array` (only inside templates, which are skipped)
            40..=52 => out.extend_from_slice(b"type "),
            _ => {
                return Err(O3dError::XFile(format!(
                    "unknown binary .x token {token} at {}",
                    p - 2
                )));
            }
        }
    }
    Ok(out)
}

fn parse_objects(t: &mut Tok, ctx: &mut Ctx, xform: Mat4) -> Result<(), O3dError> {
    while let Some(tok) = t.next() {
        match tok {
            "}" => return Ok(()),
            "template" => {
                // template Name { ... }
                t.next();
                t.expect("{")?;
                t.skip_block()?;
            }
            "Frame" => {
                // Frame [name] { ... }
                if t.peek() != Some("{") {
                    t.next();
                }
                t.expect("{")?;
                parse_frame(t, ctx, xform)?;
            }
            "Mesh" => {
                if t.peek() != Some("{") {
                    t.next();
                }
                t.expect("{")?;
                parse_mesh(t, ctx, xform)?;
            }
            "Material" => {
                let (name, material) = parse_material(t)?;
                if let Some(name) = name {
                    ctx.named_materials.insert(name, material);
                }
            }
            "AnimationSet" | "Animation" | "AnimTicksPerSecond" | "Header" => {
                if t.peek() != Some("{") {
                    t.next();
                }
                t.expect("{")?;
                t.skip_block()?;
            }
            _ => {
                // Unknown object: `Name [id] { ... }`
                if t.peek() != Some("{") {
                    t.next();
                }
                if t.peek() == Some("{") {
                    t.next();
                    t.skip_block()?;
                }
            }
        }
    }
    Ok(())
}

fn parse_frame(t: &mut Tok, ctx: &mut Ctx, parent: Mat4) -> Result<(), O3dError> {
    let mut xform = parent;
    loop {
        let tok = t
            .next()
            .ok_or_else(|| O3dError::XFile("eof in frame".into()))?;
        match tok {
            "}" => return Ok(()),
            "FrameTransformMatrix" => {
                // a data object may carry a name (`FrameTransformMatrix relative {`, the 3ds
                // Max exporter of the Solaris Urbino 18 and the Novi Sad objects)
                if t.peek() != Some("{") {
                    t.next();
                }
                t.expect("{")?;
                let mut m = [0f32; 16];
                for v in m.iter_mut() {
                    *v = t.number()?;
                }
                // consume trailing ';;' and '}'
                while let Some(x) = t.next() {
                    if x == "}" {
                        break;
                    }
                }
                // Direct3D writes the matrix row by row for row vectors (v' = v·M, the
                // translation in elements 12-14); read column by column that is already the
                // column-vector matrix glam uses. Transposing it as well dropped every
                // translation into the bottom row and turned the rotations the wrong way: the
                // BVG Citaro's Atron terminal and ALMEX (46 meshes 5.5 m forward and 1.3 m up)
                // sat at the bus origin under the floor, and the street name signs hung upside
                // down half way down their poles (their texts were turned by 180° to make up
                // for it; scene.rs no longer does).
                let local = Mat4::from_cols_array(&m);
                xform = parent * local;
            }
            "Frame" => {
                if t.peek() != Some("{") {
                    t.next();
                }
                t.expect("{")?;
                parse_frame(t, ctx, xform)?;
            }
            "Mesh" => {
                if t.peek() != Some("{") {
                    t.next();
                }
                t.expect("{")?;
                parse_mesh(t, ctx, xform)?;
            }
            "Material" => {
                let (name, material) = parse_material(t)?;
                if let Some(name) = name {
                    ctx.named_materials.insert(name, material);
                }
            }
            _ => {
                if t.peek() != Some("{") {
                    t.next();
                }
                if t.peek() == Some("{") {
                    t.next();
                    t.skip_block()?;
                }
            }
        }
    }
}

fn parse_mesh(t: &mut Tok, ctx: &mut Ctx, xform: Mat4) -> Result<(), O3dError> {
    let base_v = ctx.mesh.vertices.len() as u32;
    let base_m = ctx.mesh.materials.len() as u16;
    let nv = t.int()?;
    // (counts from the file only reserve what the text can hold)
    let room = t.s.len().saturating_sub(t.p);
    let mut positions = Vec::with_capacity(nv.min(room));
    for _ in 0..nv {
        let p = Vec3::new(t.number()?, t.number()?, t.number()?);
        positions.push(xform.transform_point3(p));
    }
    let nf = t.int()?;
    let mut faces: Vec<Vec<u32>> = Vec::with_capacity(nf.min(room));
    for _ in 0..nf {
        let n = t.int()?;
        let mut f = Vec::with_capacity(n.min(64));
        for _ in 0..n {
            f.push(t.int()? as u32);
        }
        faces.push(f);
    }
    let mut normals: Vec<Vec3> = vec![Vec3::Z; nv];
    let mut has_normal: Vec<bool> = vec![false; nv];
    let mut uvs: Vec<Vec2> = vec![Vec2::ZERO; nv];
    let mut has_uvs = false;
    let mut decl_uvs: Option<Vec<Vec2>> = None;
    let mut face_mats: Vec<u16> = vec![0; nf];
    let mut mats: Vec<Material> = Vec::new();
    let mut material_refs: Vec<(usize, String)> = Vec::new();
    loop {
        let tok = t
            .next()
            .ok_or_else(|| O3dError::XFile("eof in mesh".into()))?;
        match tok {
            "}" => break,
            "MeshNormals" => {
                if t.peek() != Some("{") {
                    t.next();
                }
                t.expect("{")?;
                let nn = t.int()?;
                let mut nrm = Vec::with_capacity(nn.min(t.s.len().saturating_sub(t.p)));
                let linear = Mat3::from_mat4(xform);
                let normal_matrix = if linear.determinant().abs() > 1e-12 {
                    linear.inverse().transpose()
                } else {
                    linear
                };
                for _ in 0..nn {
                    let n = Vec3::new(t.number()?, t.number()?, t.number()?);
                    nrm.push((normal_matrix * n).normalize_or_zero());
                }
                let nfn = t.int()?;
                for fi in 0..nfn {
                    let n = t.int()?;
                    for k in 0..n {
                        let ni = t.int()?;
                        if fi < faces.len() && k < faces[fi].len() {
                            let vi = faces[fi][k] as usize;
                            if vi < normals.len() && ni < nrm.len() {
                                normals[vi] = nrm[ni];
                                has_normal[vi] = true;
                            }
                        }
                    }
                }
                skip_to_close(t)?;
            }
            "MeshTextureCoords" => {
                if t.peek() != Some("{") {
                    t.next();
                }
                t.expect("{")?;
                let n = t.int()?;
                for i in 0..n {
                    let uv = Vec2::new(t.number()?, t.number()?);
                    if i < uvs.len() {
                        uvs[i] = uv;
                    }
                }
                has_uvs = true;
                skip_to_close(t)?;
            }
            "DeclData" => {
                if t.peek() != Some("{") {
                    t.next();
                }
                t.expect("{")?;
                decl_uvs = parse_decl_uvs(t, nv)?;
                skip_to_close(t)?;
            }
            "MeshMaterialList" => {
                if t.peek() != Some("{") {
                    t.next();
                }
                t.expect("{")?;
                let _nm = t.int()?;
                let nfi = t.int()?;
                for i in 0..nfi {
                    let m = t.int()? as u16;
                    if i < face_mats.len() {
                        face_mats[i] = m;
                    }
                }
                loop {
                    let tk = t
                        .next()
                        .ok_or_else(|| O3dError::XFile("eof in matlist".into()))?;
                    match tk {
                        "}" => break,
                        "Material" => {
                            let (name, material) = parse_material(t)?;
                            if let Some(name) = name {
                                ctx.named_materials.insert(name, material.clone());
                            }
                            mats.push(material);
                        }
                        "{" => {
                            // Keep this slot even if the named material is defined later or
                            // absent: dropping it changes every following face material index.
                            let name = t.next().ok_or_else(|| {
                                O3dError::XFile("eof in material reference".into())
                            })?;
                            if matches!(name, "{" | "}") {
                                return Err(O3dError::XFile(
                                    "missing material reference name".into(),
                                ));
                            }
                            material_refs.push((mats.len(), name.to_string()));
                            mats.push(Material::default());
                            t.skip_block()?;
                        }
                        _ => {}
                    }
                }
            }
            _ => {
                if t.peek() != Some("{") {
                    t.next();
                }
                if t.peek() == Some("{") {
                    t.next();
                    t.skip_block()?;
                }
            }
        }
    }
    if mats.is_empty() {
        mats.push(Material::default());
    }
    if let (Some(decl), false) = (decl_uvs, has_uvs) {
        uvs = decl;
    }

    if has_normal.iter().any(|h| !*h) {
        let mut acc = vec![Vec3::ZERO; nv];
        for f in &faces {
            for k in 1..f.len().saturating_sub(1) {
                let i = [f[0] as usize, f[k] as usize, f[k + 1] as usize];
                if i.iter().any(|&v| v >= nv) {
                    continue;
                }
                let p = i.map(|v| positions[v]);
                let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
                for c in 0..3 {
                    let e1 = (p[(c + 1) % 3] - p[c]).normalize_or_zero();
                    let e2 = (p[(c + 2) % 3] - p[c]).normalize_or_zero();
                    acc[i[c]] += n * e1.dot(e2).clamp(-1.0, 1.0).acos();
                }
            }
        }
        for v in 0..nv {
            if !has_normal[v] && acc[v].length_squared() > 0.0 {
                normals[v] = acc[v].normalize();
            }
        }
    }
    for i in 0..nv {
        ctx.mesh.vertices.push(Vertex {
            position: positions[i],
            normal: normals[i],
            uv: uvs[i],
        });
    }
    for (fi, f) in faces.iter().enumerate() {
        let m = base_m + face_mats[fi].min(mats.len() as u16 - 1);
        for k in 1..f.len().saturating_sub(1) {
            ctx.mesh.triangles.push(Triangle {
                indices: [base_v + f[0], base_v + f[k], base_v + f[k + 1]],
                material: m,
            });
        }
    }
    let material_base = ctx.mesh.materials.len();
    ctx.material_refs.extend(
        material_refs
            .into_iter()
            .map(|(slot, name)| (material_base + slot, name)),
    );
    ctx.mesh.materials.extend(mats);
    Ok(())
}

/// TEXCOORD0 of a `DeclData` block (after its '{'): D3D vertex elements (type, method, usage,
/// usage index), then each vertex's data as DWORDs. None without a FLOAT2 TEXCOORD0 or with an
/// element type of unknown size.
fn parse_decl_uvs(t: &mut Tok, nv: usize) -> Result<Option<Vec<Vec2>>, O3dError> {
    const TEXCOORD: u32 = 5;
    const FLOAT2: u32 = 1;
    let n = t.dword()?;
    let mut stride = 0usize;
    let mut uv_at = None;
    let mut sizes_known = true;
    for _ in 0..n {
        let (ty, _method, usage, index) = (t.dword()?, t.dword()?, t.dword()?, t.dword()?);
        // DWORDs per D3DDECLTYPE 0..=16 (FLOAT1 .. FLOAT16_4)
        let size = match ty {
            0 | 4 | 5 | 6 | 8 | 9 | 11 | 13 | 14 | 15 => 1,
            1 | 7 | 10 | 12 | 16 => 2,
            2 => 3,
            3 => 4,
            _ => {
                sizes_known = false;
                0
            }
        };
        if ty == FLOAT2 && usage == TEXCOORD && index == 0 && uv_at.is_none() {
            uv_at = Some(stride);
        }
        stride += size;
    }
    let count = t.dword()? as usize;
    let Some(at) = uv_at.filter(|_| sizes_known) else {
        return Ok(None);
    };
    let mut uvs = vec![Vec2::ZERO; nv];
    for i in 0..count {
        let d = t.dword()?;
        let (v, k) = (i / stride, i % stride);
        if v < nv && (k == at || k == at + 1) {
            uvs[v][k - at] = f32::from_bits(d);
        }
    }
    Ok(Some(uvs))
}

/// A material declaration after its `Material` token, either inline or a named object.
fn parse_material(t: &mut Tok) -> Result<(Option<String>, Material), O3dError> {
    let name = if t.peek() != Some("{") {
        t.next().map(str::to_string)
    } else {
        None
    };
    t.expect("{")?;
    let diffuse = [t.number()?, t.number()?, t.number()?, t.number()?];
    let power = t.number()?;
    let specular = [t.number()?, t.number()?, t.number()?];
    let emissive = [t.number()?, t.number()?, t.number()?];
    let mut texture = String::new();
    loop {
        match t
            .next()
            .ok_or_else(|| O3dError::XFile("eof in material".into()))?
        {
            "}" => break,
            "TextureFilename" => {
                if t.peek() != Some("{") {
                    t.next();
                }
                t.expect("{")?;
                if let Some(q) = t.next() {
                    texture = q.trim_matches('"').to_string();
                }
                skip_to_close(t)?;
            }
            "{" => t.skip_block()?,
            _ => {}
        }
    }
    Ok((
        name,
        Material {
            diffuse,
            specular,
            emissive,
            specular_power: power,
            texture,
        },
    ))
}

fn skip_to_close(t: &mut Tok) -> Result<(), O3dError> {
    let mut depth = 1;
    while depth > 0 {
        match t.next() {
            Some("{") => depth += 1,
            Some("}") => depth -= 1,
            Some(_) => {}
            None => return Err(O3dError::XFile("eof".into())),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires installed traffic-light assets; set OMSI_ROOT or OMSI_TEST_CONTENT"]
    fn installed_traffic_light_material_references() {
        let Some(root) =
            std::env::var_os("OMSI_ROOT").or_else(|| std::env::var_os("OMSI_TEST_CONTENT"))
        else {
            eprintln!("skipped: set OMSI_ROOT or OMSI_TEST_CONTENT to the installed content root");
            return;
        };
        let object = std::path::PathBuf::from(root).join("Sceneryobjects/D016_semafory");
        let mesh = crate::load_mesh(&object.join("model/semafor_3svetla.X")).unwrap();
        assert!(!mesh.materials.is_empty());
        assert!(!mesh.triangles.is_empty());
        // This housing references PDX01; another globally declared material is unused.
        // Resolving the wrong slot used to leave the housing white.
        for material in &mesh.materials {
            assert!(
                material.texture.eq_ignore_ascii_case("semafor_zaklad.bmp"),
                "unexpected housing texture: {}",
                material.texture
            );
            let texture = ::mods::resolve_path(&object.join("texture"), &material.texture);
            assert!(
                texture.is_file(),
                "missing referenced texture: {}",
                texture.display()
            );
        }
        assert!(
            mesh.triangles
                .iter()
                .all(|triangle| (triangle.material as usize) < mesh.materials.len())
        );
    }

    fn material_test_mesh(entries: &str) -> String {
        format!(
            "Mesh {{
            3; 0;0;0;, 1;0;0;, 0;1;0;;
            2; 3;0,1,2;, 3;0,2,1;;
            MeshMaterialList {{ 2; 2; 0,1;; {entries} }}
        }}"
        )
    }

    /// Traffic signals exported by 3ds Max put named materials before their frames and
    /// refer to them from each mesh. Ignoring those objects made the housings solid white.
    #[test]
    fn named_materials_keep_textures_colours_and_slot_order() {
        let mesh = material_test_mesh(
            r#"
            { SignalHousing }
            Material { 1;0;0;1;; 2; 0;0;0;; 0;0;0;; }
        "#,
        );
        let x = format!(
            r#"xof 0303txt 0032
            template Material {{ <3d82ab4d-62da-11cf-ab39-0020af71e433> FLOAT ignored; }}
            Material SignalHousing {{
                0.1;0.2;0.3;0.4;; 8; 0.5;0.6;0.7;; 0.2;0.3;0.4;;
                TextureFilename {{ "housing.bmp"; }}
            }}
            Frame Signal {{ {mesh} }}
        "#
        );
        let parsed = parse_x(x.as_bytes()).unwrap();
        assert_eq!(parsed.materials.len(), 2);
        let housing = &parsed.materials[0];
        assert_eq!(housing.texture, "housing.bmp");
        assert_eq!(housing.diffuse, [0.1, 0.2, 0.3, 0.4]);
        assert_eq!(housing.specular, [0.5, 0.6, 0.7]);
        assert_eq!(housing.emissive, [0.2, 0.3, 0.4]);
        assert_eq!(housing.specular_power, 8.0);
        assert_eq!(parsed.materials[1].diffuse, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(
            parsed
                .triangles
                .iter()
                .map(|t| t.material)
                .collect::<Vec<_>>(),
            [0, 1]
        );
    }

    #[test]
    fn material_references_resolve_forward_across_frames_and_inline_definitions() {
        let first = material_test_mesh("{ LaterInFrame } { LaterInline }");
        let second = material_test_mesh(
            r#"
            { LaterInFrame }
            Material LaterInline {
                1;1;1;1;; 1; 0;0;0;; 1;1;1;;
                TextureFilename { "lamp.bmp"; }
            }
        "#,
        );
        let x = format!(
            r#"xof 0303txt 0032
            Frame First {{ {first} }}
            Frame Second {{
                Material LaterInFrame {{
                    1;1;1;1;; 1; 0;0;0;; 0;0;0;;
                    TextureFilename {{ "housing.bmp"; }}
                }}
                Frame Child {{ {second} }}
            }}
        "#
        );
        let parsed = parse_x(x.as_bytes()).unwrap();
        let textures: Vec<&str> = parsed
            .materials
            .iter()
            .map(|m| m.texture.as_str())
            .collect();
        assert_eq!(
            textures,
            ["housing.bmp", "lamp.bmp", "housing.bmp", "lamp.bmp"]
        );
        assert_eq!(
            parsed
                .triangles
                .iter()
                .map(|t| t.material)
                .collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
    }

    #[test]
    fn missing_material_reference_preserves_following_slots() {
        let mesh = material_test_mesh(
            r#"
            { NotDeclared }
            Material { 1;0;0;1;; 1; 0;0;0;; 0;0;0;; }
        "#,
        );
        let parsed = parse_x(format!("xof 0303txt 0032\n{mesh}").as_bytes()).unwrap();
        assert_eq!(parsed.materials[0], Material::default());
        assert_eq!(parsed.materials[1].diffuse, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(
            parsed
                .triangles
                .iter()
                .map(|t| t.material)
                .collect::<Vec<_>>(),
            [0, 1]
        );
    }

    /// A Blender export: the root frame swaps Y and Z, the child frame scales, turns about
    /// its up axis and moves the mesh 5.5 m forward and 1.3 m up (the Atron terminal's frames).
    #[test]
    fn frame_matrices_are_row_major() {
        let x = "xof 0303txt 0032\n\
Frame Root {\n FrameTransformMatrix {\n 1.0, 0.0, 0.0, 0.0,\n 0.0,-0.0, 1.0, 0.0,\n 0.0, 1.0, 0.0, 0.0,\n 0.0, 0.0, 0.0, 1.0;;\n }\n\
 Frame Child {\n  FrameTransformMatrix {\n   0.0, 2.0, 0.0, 0.0,\n  -2.0, 0.0, 0.0, 0.0,\n   0.0, 0.0, 2.0, 0.0,\n  -0.5, 5.5, 1.3, 1.0;;\n  }\n\
  Mesh {\n   3;\n   1.0;0.0;0.0;,\n   0.0;1.0;0.0;,\n   0.0;0.0;1.0;;\n   1;\n   3;0,1,2;;\n\
   MeshNormals {\n    1;\n    1.0;0.0;0.0;;\n    1;\n    3;0,0,0;;\n   }\n  }\n }\n}\n";
        let m = parse_x(x.as_bytes()).unwrap();
        let p: Vec<Vec3> = m.vertices.iter().map(|v| v.position).collect();
        // Blender (x, y, z) -> child: (x, y, z)·M = (-2y, 2x, 2z) + (-0.5, 5.5, 1.3), then the
        // root swaps y and z.
        let want = [
            Vec3::new(-0.5, 1.3, 7.5),
            Vec3::new(-2.5, 1.3, 5.5),
            Vec3::new(-0.5, 3.3, 5.5),
        ];
        for (a, b) in p.iter().zip(want) {
            assert!((*a - b).length() < 1e-5, "{p:?}");
        }
        // the +x normal turns to Blender +y, D3D +z
        assert!(
            (m.vertices[0].normal - Vec3::Z).length() < 1e-5,
            "{:?}",
            m.vertices[0].normal
        );
    }

    /// A Blender export: the root frame swaps y and z, the child frame turns, scales and
    /// moves the mesh (Direct3D row-vector matrices).
    #[test]
    fn frames_move_and_turn_like_direct3d() {
        let x = "xof 0303txt 0032\nFrame Root {\n FrameTransformMatrix {\n 1,0,0,0,\n 0,0,1,0,\n 0,1,0,0,\n 0,0,0,1;;\n }\n Frame Box {\n FrameTransformMatrix {\n 2,0,0,0,\n 0,0,2,0,\n 0,-2,0,0,\n 0.5,0.25,1.5,1;;\n }\n Mesh {\n 3;\n 0;0;0;,\n 1;0;0;,\n 0;1;0;;\n 1;\n 3;0,1,2;;\n }\n }\n}\n";
        let m = parse_x(x.as_bytes()).unwrap();
        let p: Vec<Vec3> = m.vertices.iter().map(|v| v.position).collect();
        // v * child: (x, y, z) -> (2x + 0.5, -2z + 0.25, 2y + 1.5); then y and z swap
        assert!(
            (p[0] - Vec3::new(0.5, 1.5, 0.25)).length() < 1e-5,
            "{:?}",
            p[0]
        );
        assert!(
            (p[1] - Vec3::new(2.5, 1.5, 0.25)).length() < 1e-5,
            "{:?}",
            p[1]
        );
        assert!(
            (p[2] - Vec3::new(0.5, 3.5, 0.25)).length() < 1e-5,
            "{:?}",
            p[2]
        );
    }

    /// A frame that scales unevenly (the Ruede Trafohaus: 1.5 x 0.75 x 0.85) keeps the
    /// normals on their faces: they go by the inverse transpose (the frame matrix itself put
    /// the transformer house's normals 3.8° off its walls).
    #[test]
    fn uneven_scale_keeps_normals_on_their_faces() {
        let x = "xof 0303txt 0032\nFrame Box {\n FrameTransformMatrix {\n 3,0,0,0,\n 0,1,0,0,\n 0,0,1,0,\n 0,0,0,1;;\n }\n Mesh {\n 3;\n 1;0;0;,\n 0;1;0;,\n 0;0;1;;\n 1;\n 3;0,1,2;;\n MeshNormals {\n 1;\n 0.57735;0.57735;0.57735;;\n 1;\n 3;0,0,0;;\n }\n }\n}\n";
        let m = parse_x(x.as_bytes()).unwrap();
        let [a, b, c] = [0, 1, 2].map(|i| m.vertices[i].position);
        let face = (b - a).cross(c - a).normalize();
        for v in &m.vertices {
            assert!(
                v.normal.normalize().dot(face).abs() > 0.9999,
                "{:?} vs {face:?}",
                v.normal
            );
        }
    }

    /// Some exports (PAD-Labs houses on Köln) keep the UVs only in `DeclData`: skipping it put
    /// every vertex at UV 0,0 and drew the houses in one colour.
    #[test]
    fn decl_data_carries_the_texture_coordinates() {
        let uv = [(0.25f32, 0.5f32), (1.0, -0.75), (0.125, 2.0)];
        // NORMAL (FLOAT3), TEXCOORD0 (FLOAT2), TEXCOORD1 (FLOAT2): 7 DWORDs a vertex
        let data: Vec<String> = uv
            .iter()
            .flat_map(|(u, v)| {
                [0.0f32, 0.0, 1.0, *u, *v, 9.0, 9.0].map(|f| f.to_bits().to_string())
            })
            .collect();
        let decl = format!(
            "DeclData {{ 3; 2;0;3;0;, 1;0;5;0;, 1;0;5;1;; 21; {}; }}",
            data.join(",\n")
        );
        let mesh = |extra: &str| {
            format!("xof 0303txt 0032\nMesh {{ 3; 0;0;0;, 1;0;0;, 0;1;0;; 1; 3;0,1,2;; {extra} }}")
        };
        let m = parse_x(mesh(&decl).as_bytes()).unwrap();
        let got: Vec<(f32, f32)> = m.vertices.iter().map(|v| (v.uv.x, v.uv.y)).collect();
        assert_eq!(got, uv);
        // MeshTextureCoords, where a file has both, stays what it was
        let both = format!("MeshTextureCoords {{ 3; 0;0;, 0;1;, 1;1;; }} {decl}");
        let m = parse_x(mesh(&both).as_bytes()).unwrap();
        assert_eq!(m.vertices[1].uv, Vec2::new(0.0, 1.0));
        // an element type of unknown size leaves the UVs alone instead of misreading them
        let odd = decl.replacen("2;0;3;0;", "99;0;3;0;", 1);
        let m = parse_x(mesh(&odd).as_bytes()).unwrap();
        assert!(m.vertices.iter().all(|v| v.uv == Vec2::ZERO));
    }

    /// Some PAD-Labs houses on Köln are binary `.x` files; rejecting them left the houses
    /// out of the map without a word.
    #[test]
    fn binary_files_are_read() {
        let mut b = b"xof 0303bin 0032".to_vec();
        let word = |b: &mut Vec<u8>, t: u16| b.extend_from_slice(&t.to_le_bytes());
        let dword = |b: &mut Vec<u8>, v: u32| b.extend_from_slice(&v.to_le_bytes());
        let name = |b: &mut Vec<u8>, s: &str| {
            word(b, 1);
            dword(b, s.len() as u32);
            b.extend_from_slice(s.as_bytes());
        };
        let ints = |b: &mut Vec<u8>, v: &[u32]| {
            word(b, 6);
            dword(b, v.len() as u32);
            v.iter().for_each(|x| dword(b, *x));
        };
        let floats = |b: &mut Vec<u8>, v: &[f32]| {
            word(b, 7);
            dword(b, v.len() as u32);
            v.iter().for_each(|x| dword(b, x.to_bits()));
        };
        // template Skipped { <guid> DWORD x; }
        word(&mut b, 31);
        name(&mut b, "Skipped");
        word(&mut b, 10);
        word(&mut b, 5);
        b.extend_from_slice(&[7; 16]);
        word(&mut b, 41);
        name(&mut b, "x");
        word(&mut b, 20);
        word(&mut b, 11);
        // Material House 2 { ...; TextureFilename { "house.jpg"; } } (a name with a space)
        name(&mut b, "Material");
        name(&mut b, "House 2");
        word(&mut b, 10);
        floats(
            &mut b,
            &[1.0, 1.0, 1.0, 1.0, 4.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        );
        name(&mut b, "TextureFilename");
        word(&mut b, 10);
        word(&mut b, 2);
        dword(&mut b, 9);
        b.extend_from_slice(b"house.jpg");
        word(&mut b, 20);
        word(&mut b, 11);
        word(&mut b, 11);
        // Mesh { 3 vertices; 1 face; MeshTextureCoords; MeshMaterialList { { House 2 } } }
        name(&mut b, "Mesh");
        word(&mut b, 10);
        ints(&mut b, &[3]);
        floats(&mut b, &[0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 3.0, 0.0]);
        ints(&mut b, &[1, 3, 0, 1, 2]);
        name(&mut b, "MeshTextureCoords");
        word(&mut b, 10);
        ints(&mut b, &[3]);
        floats(&mut b, &[0.0, 0.0, 0.5, 0.0, 0.0, 0.25]);
        word(&mut b, 11);
        name(&mut b, "MeshMaterialList");
        word(&mut b, 10);
        ints(&mut b, &[1, 1, 0]);
        word(&mut b, 10);
        name(&mut b, "House 2");
        word(&mut b, 11);
        word(&mut b, 11);
        word(&mut b, 11);

        let m = parse_x(&b).unwrap();
        let p: Vec<Vec3> = m.vertices.iter().map(|v| v.position).collect();
        assert_eq!(
            p,
            [
                Vec3::ZERO,
                Vec3::new(2.0, 0.0, 0.0),
                Vec3::new(0.0, 3.0, 0.0)
            ]
        );
        assert_eq!(m.vertices[1].uv, Vec2::new(0.5, 0.0));
        assert_eq!(m.vertices[2].uv, Vec2::new(0.0, 0.25));
        assert_eq!(m.triangles.len(), 1);
        assert_eq!(m.materials.len(), 1);
        assert_eq!(m.materials[0].texture, "house.jpg");
        assert_eq!(m.materials[0].specular_power, 4.0);
        // a cut-off file is an error, not a panic
        assert!(parse_x(&b[..b.len() - 7]).is_err());
    }
}

#[cfg(test)]
mod damaged_tests {
    use super::*;

    #[test]
    fn damaged_files_are_errors_not_panics() {
        assert!(parse_x(b"xof 03").is_err());
        assert!(parse_x(b"xof 0302\xe9\xe9\xe9\xe9 0032 Mesh {").is_err());
        // a face naming vertex 7 of 3
        let m = parse_x(b"xof 0302txt 0032\nMesh m {\n3;\n0;0;0;,\n1;0;0;,\n0;1;0;;\n2;\n3;0,1,2;,\n3;0,1,7;;\n}\n").unwrap();
        assert_eq!(m.triangles.len(), 1);
    }
}