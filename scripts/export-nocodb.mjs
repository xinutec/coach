// One-time migration tool: pull the NocoDB "Workout" base into clean-schema
// bundles for coach. NocoDB is being retired; this is the last export.
//
// Output:
//   data/catalog/            (committed, global reference library — no user data)
//     equipment.json         equipment catalog (slug, name, category)
//     muscle-groups.json     anatomical groups (slug, name, region)
//     muscles.json           muscles (slug, name, group, function)
//     exercises.json         exercises (slug, name, variation, pattern, metric,
//                            position, unilateral, cue, demoUrl, summary,
//                            muscles[{slug,role}], equipment[slug], image{file,type})
//     images/<slug>.<ext>    exercise demo images (become DB blobs at seed time)
//   <SCRATCH>/nocodb-user-bundle.json   (NOT committed — Pippijn's private data)
//     history[]  {date, exerciseSlug, sets, reps, weightKg, band}
//     programs[] {name, entries[{exerciseSlug, targetReps, group, variation}]}
//
// The clean schema is designed from first principles (see migrations/0006);
// NocoDB is only a data source. This script does the lossy mapping once.
//
// Run:  NC_TOKEN=<xc-auth jwt> SCRATCH=<dir> nix develop -c node scripts/export-nocodb.mjs
// (get the token from the logged-in ChromeDebug NocoDB tab; see
//  reference_chrome_cdp_access — the localStorage `nocodb-gui-v2`.token)

import { mkdir, writeFile, rm } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const NC_BASE = process.env.NC_BASE ?? "https://nocodb.xinutec.org";
const NC_TOKEN = process.env.NC_TOKEN;
if (!NC_TOKEN) throw new Error("set NC_TOKEN (xc-auth JWT from the NocoDB tab)");
const SCRATCH = process.env.SCRATCH ?? "/tmp";

const REPO = join(dirname(fileURLToPath(import.meta.url)), "..");
const CATALOG = join(REPO, "data", "catalog");
const IMAGES = join(CATALOG, "images");

// NocoDB v2 table ids in the "Workout" base (pkotwrjhcrydgbh).
const TBL = {
  exercises: "mcfjy96ng6aj11x",
  muscles: "mpszpe0qtst1f4j",
  workouts: "m7eu8mt17bihbw9",
  programs: "m6ngzao0e9rwynp",
};

const h = { "xc-auth": NC_TOKEN };

async function fetchAll(tableId, fields) {
  const out = [];
  for (let offset = 0; ; offset += 100) {
    const q = new URLSearchParams({ limit: "100", offset: String(offset) });
    if (fields) q.set("fields", fields);
    const r = await fetch(`${NC_BASE}/api/v2/tables/${tableId}/records?${q}`, { headers: h });
    if (!r.ok) throw new Error(`${tableId} ${r.status} ${await r.text()}`);
    const j = await r.json();
    out.push(...j.list);
    if (j.pageInfo.isLastPage) return out;
  }
}

// ---- clean vocabularies ----------------------------------------------------

// The equipment catalog. `floor` in NocoDB = no equipment (bodyweight) → omitted.
const EQUIPMENT = [
  { slug: "dumbbell", name: "Dumbbell", category: "free_weight" },
  { slug: "barbell", name: "Barbell", category: "free_weight" },
  { slug: "kettlebell", name: "Kettlebell", category: "free_weight" },
  { slug: "trap_bar", name: "Trap bar", category: "free_weight" },
  { slug: "resistance_band", name: "Resistance band", category: "band" },
  { slug: "cable_machine", name: "Cable machine", category: "machine" },
  { slug: "medicine_ball", name: "Medicine ball", category: "ball" },
  { slug: "pull_up_bar", name: "Pull-up bar", category: "rig" },
  { slug: "gymnastic_rings", name: "Gymnastic rings", category: "rig" },
  { slug: "parallettes", name: "Parallettes", category: "rig" },
  { slug: "bench", name: "Bench", category: "bench" },
  { slug: "ghd", name: "GHD", category: "machine" },
  { slug: "yoga_ball", name: "Yoga ball", category: "ball" },
];
// NocoDB Resistance/Support value → equipment slug (null = bodyweight/none).
const EQUIP_MAP = {
  dumbbell: "dumbbell",
  barbell: "barbell",
  kettlebell: "kettlebell",
  "hex bar": "trap_bar",
  band: "resistance_band",
  cable: "cable_machine",
  "medicine ball": "medicine_ball",
  floor: null,
  bench: "bench",
  rings: "gymnastic_rings",
  "pull-up bar": "pull_up_bar",
  "high parallettes": "parallettes",
  GHD: "ghd",
  "yoga ball": "yoga_ball",
};

// Anatomical taxonomy: NocoDB muscle name → { group, region }. Region is one of
// chest|back|shoulders|arms|forearms|core|legs. This replaces NocoDB's single
// functional "Group" field (which conflated movement with anatomy).
const MG = {
  "Pectoralis major": ["chest", "chest"],
  "Pectoralis minor": ["chest", "chest"],
  "Serratus anterior": ["serratus", "chest"],
  "Latissimus dorsi": ["lats", "back"],
  Trapezius: ["traps", "back"],
  Rhomboids: ["upper_back", "back"],
  "Teres major": ["upper_back", "back"],
  "Erector spinae": ["lower_back", "back"],
  "Deltoid anterior": ["deltoids", "shoulders"],
  "Deltoid posterior": ["deltoids", "shoulders"],
  Infraspinatus: ["rotator_cuff", "shoulders"],
  "Teres minor": ["rotator_cuff", "shoulders"],
  Subscapularis: ["rotator_cuff", "shoulders"],
  Supraspinatus: ["rotator_cuff", "shoulders"],
  "Biceps brachii": ["biceps", "arms"],
  Brachialis: ["biceps", "arms"],
  "Triceps brachii": ["triceps", "arms"],
  Brachioradialis: ["forearms", "forearms"],
  "Wrist extensors": ["forearms", "forearms"],
  "Wrist flexors": ["forearms", "forearms"],
  "Rectus abdominis": ["abdominals", "core"],
  "External obliques": ["obliques", "core"],
  "Transversus abdominis": ["deep_core", "core"],
  "Quadratus lumborum": ["deep_core", "core"],
  Iliopsoas: ["hip_flexors", "legs"],
  Pectineus: ["hip_flexors", "legs"],
  Sartorius: ["hip_flexors", "legs"],
  "Tensor fasciae latae": ["glutes", "legs"],
  Gracilis: ["adductors", "legs"],
  "Adductor longus": ["adductors", "legs"],
  "Adductor magnus": ["adductors", "legs"],
  "Vastus lateralis": ["quadriceps", "legs"],
  "Vastus medialis": ["quadriceps", "legs"],
  "Vastus intermedius": ["quadriceps", "legs"],
  "Rectus femoris": ["quadriceps", "legs"],
  "Gluteus maximus": ["glutes", "legs"],
  "Gluteus medius": ["glutes", "legs"],
  "Biceps femoris": ["hamstrings", "legs"],
  Semitendinosus: ["hamstrings", "legs"],
  Semimembranosus: ["hamstrings", "legs"],
  Gastrocnemius: ["calves", "legs"],
  Soleus: ["calves", "legs"],
  Plantaris: ["calves", "legs"],
  "Tibialis anterior": ["lower_leg", "legs"],
  "Extensor digitorum longus": ["lower_leg", "legs"],
  "Flexor digitorum longus": ["lower_leg", "legs"],
};
const GROUP_NAMES = {
  chest: "Chest",
  serratus: "Serratus",
  lats: "Lats",
  traps: "Trapezius",
  upper_back: "Upper back",
  lower_back: "Lower back",
  deltoids: "Deltoids",
  rotator_cuff: "Rotator cuff",
  biceps: "Biceps",
  triceps: "Triceps",
  forearms: "Forearms",
  abdominals: "Abdominals",
  obliques: "Obliques",
  deep_core: "Deep core",
  hip_flexors: "Hip flexors",
  adductors: "Adductors",
  quadriceps: "Quadriceps",
  glutes: "Glutes",
  hamstrings: "Hamstrings",
  calves: "Calves",
  lower_leg: "Lower leg",
};

// Functional pattern (push/pull/legs/core) — the pacing recovery axis. Derived
// from the primary muscle's NocoDB functional group.
function patternOf(nocodbGroup) {
  if (nocodbGroup?.startsWith("Push")) return "push";
  if (nocodbGroup?.startsWith("Pull")) return "pull";
  if (nocodbGroup?.startsWith("Core")) return "core";
  if (nocodbGroup?.startsWith("Legs")) return "legs";
  return null;
}

const slugify = (s) =>
  s
    .toLowerCase()
    .normalize("NFKD")
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "");

const HOLD_RE = /\b(hold|plank|l-?sit|hang|support|lever|planche|iso)\b/i;
const UNILATERAL_RE =
  /\b(single|one[ -]arm|one[ -]leg|archer|suitcase|typewriter|seesaw|staggered|copenhagen|pistol)\b/i;

// Short label per equipment slug, for disambiguating same-named movements.
const EQUIP_LABEL = {
  dumbbell: "dumbbell", barbell: "barbell", kettlebell: "kettlebell", trap_bar: "trap bar",
  resistance_band: "band", cable_machine: "cable", medicine_ball: "med ball",
  pull_up_bar: "bar", gymnastic_rings: "rings", parallettes: "parallettes",
  bench: "bench", ghd: "GHD", yoga_ball: "ball",
};
const RESISTANCE = new Set(["dumbbell", "barbell", "kettlebell", "trap_bar", "resistance_band", "cable_machine", "medicine_ball"]);
// The equipment that best distinguishes a movement variant: the resistance
// implement if any, else the support apparatus.
function distinguisher(ex) {
  const resist = ex.equipment.find((s) => RESISTANCE.has(s));
  const support = ex.equipment.find((s) => !RESISTANCE.has(s));
  const parts = [];
  if (resist) parts.push(EQUIP_LABEL[resist]);
  if (support && !resist) parts.push(EQUIP_LABEL[support]);
  else if (support && support !== "bench") parts.push(EQUIP_LABEL[support]);
  if (ex.position) parts.push(ex.position);
  return parts.join(", ");
}

const ext = (mime, title) => {
  const m = { "image/png": "png", "image/jpeg": "jpg", "image/webp": "webp", "image/gif": "gif" };
  if (m[mime]) return m[mime];
  const t = (title ?? "").match(/\.([a-z0-9]+)$/i);
  return t ? t[1].toLowerCase() : "img";
};

async function main() {
  await rm(CATALOG, { recursive: true, force: true });
  await mkdir(IMAGES, { recursive: true });

  // --- muscles + groups ---
  const rawMuscles = await fetchAll(TBL.muscles, "Id,Name,Group,Function");
  const muscleById = new Map(); // NocoDB muscle name → slug + nocodb group
  const groupsUsed = new Map();
  const muscles = [];
  for (const m of rawMuscles) {
    const map = MG[m.Name];
    if (!map) throw new Error(`unmapped muscle: ${m.Name}`);
    const [group, region] = map;
    groupsUsed.set(group, region);
    const slug = slugify(m.Name);
    muscles.push({ slug, name: m.Name, group, function: m.Function ?? null });
    muscleById.set(m.Name, { slug, nocodbGroup: m.Group });
  }
  const muscleGroups = [...groupsUsed].map(([slug, region]) => ({
    slug,
    name: GROUP_NAMES[slug],
    region,
  }));

  // --- exercises ---
  const rawEx = await fetchAll(
    TBL.exercises,
    "Id,Name,Variation,Position,Resistance,Support,Comment,Summary,Video,Count," +
      "Primary Muscle Names,Secondary Muscle Names,Picture",
  );
  const exercises = [];
  for (const e of rawEx) {
    const variation = e.Variation || null;

    const prim = (e["Primary Muscle Names"] ?? []).map((n) => ({ slug: muscleById.get(n)?.slug, role: "primary", raw: n }));
    const sec = (e["Secondary Muscle Names"] ?? []).map((n) => ({ slug: muscleById.get(n)?.slug, role: "secondary", raw: n }));
    for (const x of [...prim, ...sec]) if (!x.slug) throw new Error(`ex ${e.Name}: unknown muscle ${x.raw}`);

    // pattern: majority functional group among primary muscles.
    const tally = {};
    for (const n of e["Primary Muscle Names"] ?? []) {
      const p = patternOf(muscleById.get(n)?.nocodbGroup);
      if (p) tally[p] = (tally[p] ?? 0) + 1;
    }
    const pattern = Object.entries(tally).sort((a, b) => b[1] - a[1])[0]?.[0] ?? "core";

    // equipment: union of Resistance + Support (floor → none).
    const equip = new Set();
    for (const f of [e.Resistance, e.Support]) {
      if (f == null || f === "") continue;
      const slugE = EQUIP_MAP[f];
      if (slugE === undefined) throw new Error(`ex ${e.Name}: unknown equipment '${f}'`);
      if (slugE) equip.add(slugE);
    }

    // metric (inferred default; correctable in the Library UI).
    const hasWeight = e.Resistance && ["dumbbell", "barbell", "kettlebell", "hex bar"].includes(e.Resistance);
    const metric = HOLD_RE.test(`${e.Name} ${variation ?? ""}`) ? "hold" : hasWeight ? "weighted_reps" : "reps";

    // image bytes fetched now; written after slugs are finalized below.
    let imageBuf = null;
    let imageType = null;
    let imageExt = null;
    const pic = e.Picture?.[0];
    if (pic) {
      const url = pic.signedPath ? `${NC_BASE}/${pic.signedPath}` : `${NC_BASE}/${pic.path}`;
      const r = await fetch(url, { headers: pic.signedPath ? {} : h });
      if (!r.ok) throw new Error(`image ${e.Name}: ${r.status}`);
      imageBuf = Buffer.from(await r.arrayBuffer());
      imageType = pic.mimetype ?? "image/jpeg";
      imageExt = ext(imageType, pic.title);
    }

    exercises.push({
      _id: e.Id,
      _imageBuf: imageBuf,
      _imageType: imageType,
      _imageExt: imageExt,
      slug: null, // finalized in the disambiguation pass
      name: e.Name,
      variation,
      pattern,
      metric,
      position: e.Position || null,
      unilateral: UNILATERAL_RE.test(`${e.Name} ${variation ?? ""}`),
      cue: e.Comment || null,
      demoUrl: e.Video || null,
      summary: e.Summary || null,
      muscles: [...prim, ...sec].map(({ slug, role }) => ({ slug, role })),
      equipment: [...equip],
      image: null,
    });
  }

  // Disambiguate movements that share a display name (name + variation) — in
  // NocoDB these are equipment variants (pull-up on bar vs rings, dumbbell vs
  // barbell bench) that must stay distinct but read unambiguously. Fold the
  // distinguishing equipment/position into the variation.
  const byDisplay = new Map();
  for (const ex of exercises) {
    const key = `${ex.name.toLowerCase()}||${(ex.variation ?? "").toLowerCase()}`;
    if (!byDisplay.has(key)) byDisplay.set(key, []);
    byDisplay.get(key).push(ex);
  }
  for (const group of byDisplay.values()) {
    if (group.length < 2) continue;
    for (const ex of group) {
      const d = distinguisher(ex);
      if (d) ex.variation = ex.variation ? `${ex.variation}, ${d}` : d;
    }
  }

  // Finalize unique slugs and write image files under the final slug.
  const exBySlug = new Map();
  const exIdToSlug = new Map();
  for (const ex of exercises) {
    const base = slugify(ex.variation ? `${ex.name} ${ex.variation}` : ex.name);
    let slug = base;
    for (let i = 2; exBySlug.has(slug); i++) slug = `${base}_${i}`;
    exBySlug.set(slug, true);
    ex.slug = slug;
    exIdToSlug.set(ex._id, slug);
    if (ex._imageBuf) {
      const file = `${slug}.${ex._imageExt}`;
      await writeFile(join(IMAGES, file), ex._imageBuf);
      ex.image = { file, type: ex._imageType };
    }
    delete ex._id;
    delete ex._imageBuf;
    delete ex._imageType;
    delete ex._imageExt;
  }

  // --- write catalog ---
  const w = (name, data) => writeFile(join(CATALOG, name), JSON.stringify(data, null, 2) + "\n");
  await w("equipment.json", EQUIPMENT);
  await w("muscle-groups.json", muscleGroups);
  await w("muscles.json", muscles);
  await w("exercises.json", exercises);

  // --- user bundle (private; scratchpad) ---
  const rawW = await fetchAll(TBL.workouts, "Date,Sets,Count,Weight (kg),Band,Exercise");
  const history = rawW
    .filter((r) => r.Exercise)
    .map((r) => ({
      date: r.Date,
      exerciseSlug: exIdToSlug.get(r.Exercise.Id),
      sets: r.Sets ?? 1,
      reps: r.Count ?? null,
      weightKg: r["Weight (kg)"] ?? null,
      band: r.Band ?? null,
    }));

  const rawP = await fetchAll(TBL.programs, "Name,Group,Variation,Count,Exercise");
  const byName = new Map();
  for (const p of rawP) {
    if (!p.Exercise) continue;
    if (!byName.has(p.Name)) byName.set(p.Name, []);
    byName.get(p.Name).push({
      exerciseSlug: exIdToSlug.get(p.Exercise.Id),
      targetReps: p.Count ?? null,
      group: p.Group ?? null,
      variation: p.Variation ?? null,
    });
  }
  const programs = [...byName].map(([name, entries]) => ({ name, entries }));

  await writeFile(
    join(SCRATCH, "nocodb-user-bundle.json"),
    JSON.stringify({ history, programs }, null, 2) + "\n",
  );

  const unknownSlugs = [...exIdToSlug.values()];
  console.log(
    JSON.stringify(
      {
        muscleGroups: muscleGroups.length,
        muscles: muscles.length,
        equipment: EQUIPMENT.length,
        exercises: exercises.length,
        images: exercises.filter((e) => e.image).length,
        historyRows: history.length,
        programs: programs.length,
        historyMissingExercise: history.filter((r) => !r.exerciseSlug).length,
        exampleSlugs: unknownSlugs.slice(0, 5),
      },
      null,
      2,
    ),
  );
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
