# blender -b --factory-startup -P blender_export.py -- <in.fbx | person.json> <out prefix> <out.json>
# (a person.json is made with MPFB, which must be installed in the Blender profile used)

import json
import re
import struct
import sys

import bmesh
import bpy
from mathutils import Matrix, Vector
from mathutils.bvhtree import BVHTree

# triangles of each level at most, besides the small parts kept whole: the first keeps a
# Rocketbox figure as it is
LODS = [("", 12000), ("_mid", 2700), ("_low", 700)]
HAIR = re.compile("hair", re.I)
OMSI_BONES = ["OS_L", "OS_R", "US_L", "US_R", "OA_L", "OA_R", "UA_L", "UA_R",
              "Hip", "Main", "Head", "Hand_L", "Hand_R"]
FINGERS = ("thumb_", "index_", "middle_", "ring_", "pinky_")
SMALL_PART = 0.04


def omsi_bone_of(name):
    n = re.sub(r"^Bip\d+", "", name).strip()
    for side in ("L", "R"):
        s = side.lower()
        if n in (f"{side} Thigh", f"thigh_{s}"):
            return f"OS_{side}"
        if n in (f"{side} Calf", f"{side} Foot", f"calf_{s}", f"foot_{s}", f"ball_{s}") \
                or n.startswith(f"{side} Toe"):
            return f"US_{side}"
        if n in (f"{side} UpperArm", f"upperarm_{s}"):
            return f"OA_{side}"
        if n in (f"{side} Forearm", f"lowerarm_{s}"):
            return f"UA_{side}"
        if n in (f"{side} Hand", f"hand_{s}") or n.startswith(f"{side} Finger") \
                or (n.startswith(FINGERS) and n.endswith(f"_{s}")):
            return f"Hand_{side}"
        if n in (f"{side} Clavicle", f"clavicle_{s}"):
            return "Main"
    if n in ("Pelvis", "pelvis"):
        return "Hip"
    if n.startswith(("Spine", "spine_")) or n in ("Neck", "neck_01"):
        return "Main"
    if n in ("Head", "head"):
        return "Head"
    return None


def skeleton(arm):
    """The bones `main` measures and poses, by role, in this armature's naming."""
    if "thigh_r" in arm.data.bones:
        return {"thigh": "thigh_{s}", "calf": "calf_{s}", "waist": "spine_02",
                "upperarm": "upperarm_{s}", "forearm": "lowerarm_{s}", "head": "head",
                "hand": "hand_{s}", "finger": "middle_01_{s}"}
    bip = next(b.name for b in arm.data.bones if b.parent is None).split()[0]
    return {"thigh": bip + " {S} Thigh", "calf": bip + " {S} Calf", "waist": bip + " Spine1",
            "upperarm": bip + " {S} UpperArm", "forearm": bip + " {S} Forearm",
            "head": bip + " Head", "hand": bip + " {S} Hand", "finger": bip + " {S} Finger2"}


def resolve(arm, name):
    b = arm.data.bones.get(name)
    while b is not None:
        o = omsi_bone_of(b.name)
        if o:
            return o
        b = b.parent
    return None


def aim(arm, bone, toward, target):
    bpy.context.view_layer.update()
    a = arm.matrix_world
    pb = arm.pose.bones[bone]
    head = a @ pb.head
    cur = (a @ arm.pose.bones[toward].head - head).normalized()
    q = cur.rotation_difference(target)
    r = Matrix.Translation(head) @ q.to_matrix().to_4x4() @ Matrix.Translation(-head)
    pb.matrix = a.inverted() @ r @ a @ pb.matrix
    bpy.context.view_layer.update()


def to_omsi(v):
    # Rocketbox and MakeHuman face -y with their left side at +x; OMSI's people face +y
    # with the right at +x.
    return Vector((-v.x, -v.y, v.z))


def texture_of(mat):
    """The picture the material's colour comes from, searched upstream of Base Color."""
    if not mat or not mat.use_nodes:
        return None
    for n in mat.node_tree.nodes:
        if n.type != "BSDF_PRINCIPLED" or not n.inputs["Base Color"].links:
            continue
        todo, seen, found = [n.inputs["Base Color"].links[0].from_node], set(), []
        while todo:
            node = todo.pop(0)
            if node.name in seen:
                continue
            seen.add(node.name)
            img = getattr(node, "image", None)
            if node.type == "TEX_IMAGE" and img and img.filepath:
                found.append(bpy.path.abspath(img.filepath))
            for i in node.inputs:
                todo.extend(link.from_node for link in i.links)
        # (MakeHuman's materials mix an occlusion map into the colour)
        found.sort(key=lambda f: ("diffuse" not in f.lower(), "_ao" in f.lower()))
        if found:
            return found[0]
    return None


def small_part_vertices(ob):
    bm = bmesh.new()
    bm.from_mesh(ob.data)
    m = ob.matrix_world
    small, seen = set(), set()
    for v in bm.verts:
        if v.index in seen:
            continue
        seen.add(v.index)
        part, todo = [], [v]
        while todo:
            x = todo.pop()
            part.append(x)
            for e in x.link_edges:
                o = e.other_vert(x)
                if o.index not in seen:
                    seen.add(o.index)
                    todo.append(o)
        pts = [m @ p.co for p in part]
        if max(max(p[i] for p in pts) - min(p[i] for p in pts) for i in range(3)) <= SMALL_PART:
            small.update(p.index for p in part)
    count = len(bm.verts)
    bm.free()
    return small, count


def without_vertices(ob, drop):
    bm = bmesh.new()
    bm.from_mesh(ob.data)
    bmesh.ops.delete(bm, geom=[v for v in bm.verts if drop(v.index)], context="VERTS")
    bm.to_mesh(ob.data)
    bm.free()


def decimated(ob, ratio):
    c = ob.copy()
    c.data = ob.data.copy()
    bpy.context.scene.collection.objects.link(c)
    with bpy.context.temp_override(object=c, active_object=c, selected_objects=[c]):
        if c.data.shape_keys:
            bpy.ops.object.shape_key_remove(all=True, apply_mix=True)
        # the body under the clothes goes before the armature bends what is left
        for mod in [m for m in c.modifiers if m.type == "MASK"]:
            bpy.ops.object.modifier_move_to_index(modifier=mod.name, index=0)
            bpy.ops.object.modifier_apply(modifier=mod.name)
    if ratio >= 1.0:
        return [c]
    small, count = small_part_vertices(c)
    if len(small) == count:
        return [c]
    out = [c]
    if small:
        part = c.copy()
        part.data = c.data.copy()
        bpy.context.scene.collection.objects.link(part)
        without_vertices(part, lambda i: i not in small)
        without_vertices(c, lambda i: i in small)
        out.append(part)
    with bpy.context.temp_override(object=c, active_object=c, selected_objects=[c]):
        mod = c.modifiers.new("lod", "DECIMATE")
        mod.ratio = ratio
        mod.use_collapse_triangulate = True
        # decimated before the armature bends it, so the weights still belong to the vertices
        bpy.ops.object.modifier_move_to_index(modifier=mod.name, index=0)
        bpy.ops.object.modifier_apply(modifier=mod.name)
    return out


def is_hair(ob):
    return any(HAIR.search(texture_of(m) or "") for m in ob.data.materials)


def tuck_scalp(verts, tris, mats, textures, weights, neck):
    """Decimated apart, the scalp poked through the hair: it is pulled in under it."""
    hair_m = {k for k, name in enumerate(mats) if HAIR.search(textures.get(name) or "")}
    if not hair_m:
        return
    head = {k: w for k, w in weights["Head"]}
    hair_t = [t[:3] for t in tris if t[3] in hair_m]
    hair_v = {i for t in hair_t for i in t}
    pos = [Vector(v[:3]) for v in verts]
    top = max(p.y for p in pos)
    scalp = [i for i, p in enumerate(pos)
             if i not in hair_v and head.get(i, 0.0) > 0.5 and p.y > neck + 0.45 * (top - neck)]
    if not scalp:
        return
    centre = sum((pos[i] for i in scalp), Vector()) / len(scalp)
    centre.y = neck + 0.5 * (top - neck)
    bvh = BVHTree.FromPolygons(pos, hair_t, all_triangles=True)
    for i in scalp:
        d = pos[i] - centre
        r = d.length
        if r < 1e-4:
            continue
        d.normalize()
        hit = bvh.ray_cast(centre + d * (0.4 * r), d, 0.6 * r + 0.03)[0]
        if hit is None:
            continue
        under = (hit - centre).length - 0.006
        if under < r:
            p = centre + d * max(under, 0.6 * r)
            verts[i] = (p.x, p.y, p.z, *verts[i][3:])


def bake(arm, meshes, mats, textures):
    dg = bpy.context.evaluated_depsgraph_get()
    verts, tris = [], []
    weights = {b: [] for b in OMSI_BONES}
    hand_r = []
    for ob in meshes:
        src = ob.data
        groups = [resolve(arm, g.name) for g in ob.vertex_groups]
        ev = ob.evaluated_get(dg)
        me = ev.to_mesh()
        m = ev.matrix_world
        nm = m.to_3x3().inverted().transposed()
        me.calc_loop_triangles()
        uv = me.uv_layers.active.data
        corner_normals = me.corner_normals
        slot = []
        for mat in me.materials:
            name = mat.name if mat else ""
            if name not in mats:
                mats.append(name)
                textures[name] = texture_of(mat)
            slot.append(mats.index(name))
        per_vertex = []
        for v in src.vertices:
            acc = {}
            for g in v.groups:
                b = groups[g.group] if g.group < len(groups) else None
                if b and g.weight > 0.0:
                    acc[b] = acc.get(b, 0.0) + g.weight
            total = sum(acc.values())
            if total <= 0.0:
                acc, total = {"Main": 1.0}, 1.0
            per_vertex.append({b: w / total for b, w in acc.items() if w / total >= 0.01})
        if len(me.vertices) != len(src.vertices):
            raise SystemExit(f"{ob.name}: its modifiers change the vertices")
        index = {}
        for t in me.loop_triangles:
            ids = []
            for li, vi in zip(t.loops, t.vertices):
                p = to_omsi(m @ me.vertices[vi].co)
                n = to_omsi(nm @ corner_normals[li].vector).normalized()
                u = uv[li].uv
                key = (vi, round(u.x, 5), round(u.y, 5), round(n.x, 3), round(n.y, 3), round(n.z, 3))
                k = index.get(key)
                if k is None:
                    k = index[key] = len(verts)
                    # .o3d is Direct3D's frame (y up, z forward) with v running down the texture
                    verts.append((p.x, p.z, p.y, n.x, n.z, n.y, u.x, 1.0 - u.y))
                    for b, w in per_vertex[vi].items():
                        weights[b].append((k, w))
                    if per_vertex[vi].get("Hand_R", 0.0) > 0.5:
                        hand_r.append(p)
                ids.append(k)
            # the swap of y and z mirrors the mesh, so the winding turns round with it
            tris.append((ids[0], ids[2], ids[1], slot[t.material_index] if slot else 0))
        ev.to_mesh_clear()
    return verts, tris, weights, hand_r


def write_o3d(path, verts, tris, mats, textures, weights):
    with open(path, "wb") as f:
        f.write(bytes([0x84, 0x19, 3, 1]))
        f.write(b"\x17" + struct.pack("<I", len(verts)))
        for v in verts:
            f.write(struct.pack("<8f", *v))
        f.write(b"\x49" + struct.pack("<I", len(tris)))
        for t in tris:
            f.write(struct.pack("<3IH", *t))
        f.write(b"\x26" + struct.pack("<H", len(mats)))
        for name in mats:
            t = textures.get(name)
            tex = (re.split(r"[\\/]", t)[-1].rsplit(".", 1)[0] + ".dds" if t else "").encode("cp1252")
            f.write(struct.pack("<11f", 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 1))
            f.write(bytes([len(tex)]) + tex)
        f.write(b"\x79" + struct.pack("<16f", 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1))
        bones = [(b, w) for b, w in weights.items() if w]
        f.write(b"\x54" + struct.pack("<H", len(bones)))
        for b, ws in bones:
            if len(ws) > 0xFFFF:
                raise SystemExit(f"{path}: bone {b} weights {len(ws)} vertices, .o3d allows 65535")
            name = b.encode()
            f.write(bytes([len(name)]) + name + struct.pack("<H", len(ws)))
            for k, w in ws:
                f.write(struct.pack("<If", k, w))


def make_person(spec):
    from bl_ext.user_default.mpfb.services import HumanService
    spec = dict(spec)
    stature = spec.pop("neo_height", None)
    info = HumanService._create_default_human_info_dict()
    info.update(spec)
    settings = HumanService.get_default_deserialization_settings()
    settings["subdiv_levels"] = 0
    HumanService.deserialize_from_dict(info, settings)
    kinds = {o.name: o.get("MPFB_GEN_object_type") for o in bpy.data.objects}
    # the full body stays behind the proxy fitted to it
    if "Proxymeshes" in kinds.values():
        for o in bpy.data.objects:
            if kinds[o.name] == "Basemesh":
                bpy.data.objects.remove(o)
    # (MakeHuman's height macro made a 7-year-old 0.98 m)
    if stature:
        bpy.context.view_layer.update()
        dg = bpy.context.evaluated_depsgraph_get()
        body = [o for o in bpy.data.objects if kinds.get(o.name) in ("Proxymeshes", "Basemesh")]
        # (the feet are masked off under the shoes)
        s = stature / max((o.matrix_world @ v.co).z for o in body
                          for v in o.evaluated_get(dg).data.vertices)
        for o in bpy.data.objects:
            if o.parent is None:
                o.location *= s
                o.scale *= s
        bpy.context.view_layer.update()


def main():
    source, prefix, out_json = sys.argv[sys.argv.index("--") + 1:][:3]
    bpy.ops.wm.read_factory_settings(use_empty=True)
    if source.lower().endswith(".json"):
        with open(source) as f:
            make_person(json.load(f))
    else:
        bpy.ops.import_scene.fbx(filepath=source)
    arm = next(o for o in bpy.data.objects if o.type == "ARMATURE")
    meshes = [o for o in bpy.data.objects if o.type == "MESH"]
    bones = skeleton(arm)

    def bone(role, side="R"):
        return bones[role].format(S=side, s=side.lower())

    # OMSI animates from a T-pose; Rocketbox and MakeHuman ship an A-pose.
    for side, x in (("L", 1.0), ("R", -1.0)):
        out = Vector((x, 0.0, 0.0))
        aim(arm, bone("upperarm", side), bone("forearm", side), out)
        aim(arm, bone("forearm", side), bone("hand", side), out)
        aim(arm, bone("hand", side), bone("finger", side), out)

    mats, textures = [], {}
    full = sum(len(p.vertices) - 2 for o in meshes for p in o.data.polygons)
    levels, hand_r = [], []
    for suffix, most in LODS:
        ratio = min(1.0, most / max(full, 1))
        parts = [d for o in meshes for d in decimated(o, ratio ** 0.5 if is_hair(o) else ratio)]
        v, t, w, hands = bake(arm, parts, mats, textures)
        hand_r = hand_r or hands
        levels.append((f"{prefix}{suffix}.o3d", v, t, w))

    a = arm.matrix_world

    def joint(role):
        return to_omsi(a @ arm.pose.bones[bone(role)].head)

    for _, v, t, w in levels:
        tuck_scalp(v, t, mats, textures, w, joint("head").z)

    hip, knee, waist = joint("thigh"), joint("calf"), joint("waist")
    shoulder, elbow, neck, hand = joint("upperarm"), joint("forearm"), joint("head"), joint("hand")
    finger = Vector((max(p.x for p in hand_r), hand.y, hand.z)) if hand_r else hand
    links = [hip.x, hip.y, hip.z, knee.x, knee.y, knee.z, waist.y, waist.z,
             shoulder.x, shoulder.y, shoulder.z, elbow.x, elbow.y, elbow.z, neck.y, neck.z,
             hand.x, hand.y, hand.z, finger.x, finger.y, finger.z]
    height = max(v[1] for v in levels[0][1])

    for path, v, t, w in levels:
        write_o3d(path, v, t, mats, textures, w)

    with open(out_json, "w") as f:
        json.dump({"links": [round(x, 4) for x in links], "height": round(height, 3),
                   "materials": [{"name": m, "texture": textures.get(m)} for m in mats],
                   "levels": [{"file": re.split(r"[\\/]", path)[-1], "vertices": len(v),
                               "triangles": len(t)} for path, v, t, _ in levels]}, f)


main()
