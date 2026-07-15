export const meta = {
  name: 'stepcheck-eval-augment',
  description: 'Gold-label expansion (2 independent passes + adjudication + agreement) and in-the-wild precision triage (judge + adversarial verify) for the StepCheck paper revision',
  phases: [
    { title: 'GoldLabel' },
    { title: 'Adjudicate' },
    { title: 'Triage' },
    { title: 'Verify' },
  ],
}

const ROOT = (typeof args === 'object' && args && args.root) || process.cwd()
const ARGS = `${ROOT}/eval/wf-args.json`
const GOLD_N = (typeof args === 'object' && args && args.goldN) || 14
const TRIAGE_N = (typeof args === 'object' && args && args.triageN) || 79

const CRITERIA = `LABELLING CRITERIA (apply strictly, judge ONLY from the task's real-world operational semantics — its name, Resource ARN, FunctionName/Action, and surrounding control flow; do NOT run any tool that infers labels):
- idempotent = TRUE if repeating the operation causes NO additional external effect. Reads (get/list/describe/query/scan), validations, idempotent writes keyed by a stable id (e.g. PutItem by primary key, S3 PutObject to a fixed key), and "cancel"/"delete"/"release" operations that converge to the same end state are idempotent. A task that creates a new record each call, charges/pays, sends/publishes a message or email, appends, or increments a counter is NOT idempotent (false).
- persistent = TRUE if the task ACQUIRES or COMMITS a durable external resource that a later failure would need to compensate/undo (e.g. reserve inventory, create order/account/instance, charge a card, provision infrastructure, write a durable record that matters). FALSE for reads/validations, for notifications (SNS/SES/"notify"/"publish" are fire-and-forget, NOT persistent), and for COMPENSATORS themselves (refund/cancel/release/rollback/delete-on-cleanup are the undo action, NOT a new persistent acquisition).
- If a task's semantics cannot be determined with confidence (e.g. a generic "Call HTTP API", "Process", "Transform", "Lambda" with an opaque name), set the field to null (means: omit / abstain). Do not guess.
Pass and Choice and Wait states are NOT tasks; only label Task states (including tasks nested inside Map iterators and Parallel branches).`

const LABEL_SCHEMA = {
  type: 'object', additionalProperties: false,
  properties: {
    file: { type: 'string' },
    labels: {
      type: 'array',
      items: {
        type: 'object', additionalProperties: false,
        properties: {
          task: { type: 'string', description: 'exact state name as it appears in the JSON' },
          idempotent: { type: ['boolean', 'null'] },
          persistent: { type: ['boolean', 'null'] },
          rationale: { type: 'string', description: 'one short clause' },
        },
        required: ['task', 'idempotent', 'persistent', 'rationale'],
      },
    },
  },
  required: ['file', 'labels'],
}

const ADJ_SCHEMA = {
  type: 'object', additionalProperties: false,
  properties: {
    final: {
      type: 'array',
      items: {
        type: 'object', additionalProperties: false,
        properties: {
          task: { type: 'string' },
          idempotent: { type: ['boolean', 'null'] },
          persistent: { type: ['boolean', 'null'] },
        },
        required: ['task', 'idempotent', 'persistent'],
      },
    },
  },
  required: ['final'],
}

const TRIAGE_SCHEMA = {
  type: 'object', additionalProperties: false,
  properties: {
    verdicts: {
      type: 'array',
      items: {
        type: 'object', additionalProperties: false,
        properties: {
          state: { type: 'string' },
          code: { type: 'string' },
          verdict: { type: 'string', enum: ['TRUE_POSITIVE', 'FALSE_POSITIVE'] },
          reason: { type: 'string' },
        },
        required: ['state', 'code', 'verdict', 'reason'],
      },
    },
  },
  required: ['verdicts'],
}

function labelPrompt(i, pass) {
  return `You are independent annotator ${pass} producing a GOLD-STANDARD label set for a static-analysis paper. ${CRITERIA}

Step 1: Use the Read tool to read ${ARGS}. Parse it as JSON. Take the filename gold[${i}].
Step 2: Use the Read tool to read ${ROOT}/corpus/asl/<that filename>.
Step 3: Enumerate EVERY Task state (including nested in Map "Iterator"/"ItemProcessor" and Parallel "Branches"). For each, decide idempotent and persistent per the criteria above (true/false/null).
Return the file basename and the per-task labels. Use exact state names. Be principled and consistent; this is ground truth, not a guess.`
}

function triagePrompt(i) {
  return `You are triaging StepCheck's inference-driven WARNINGS to measure their real-world precision. A warning is a TRUE_POSITIVE if StepCheck's inferred classification is operationally correct AND the flagged risk is genuine; it is a FALSE_POSITIVE if the inferred classification is wrong for that task (e.g. a compensator like RefundPayment/CancelOrder flagged as persistent, a read flagged as a write, a notification flagged persistent) or the flag is otherwise spurious.

${CRITERIA}

SC3001 means: a NON-idempotent task is retried on a broad error class (States.ALL/States.TaskFailed) and may duplicate its effect. TRUE_POSITIVE iff the task is genuinely non-idempotent (re-running it duplicates an external effect).
SC4001 means: a PERSISTENT task has no compensation/Catch. TRUE_POSITIVE iff the task genuinely acquires a durable resource that a later failure should undo (and is not itself a compensator/notification/read).

Step 1: Read ${ARGS} (Read tool), parse JSON, take entry triage[${i}] = {f: filename, x: [[code,state],...]}.
Step 2: Read ${ROOT}/corpus/asl/<f>.
Step 3: For EACH [code,state] in x, judge TRUE_POSITIVE or FALSE_POSITIVE with a one-clause reason grounded in the task's real semantics. Return all verdicts.`
}

// ---------- Phase 1+2: gold labelling ----------
phase('GoldLabel')
const goldResults = await pipeline(
  Array.from({ length: GOLD_N }, (_, i) => i),
  async (i) => {
    const [a, b] = await parallel([
      () => agent(labelPrompt(i, 'A'), { label: `label-A:${i}`, phase: 'GoldLabel', schema: LABEL_SCHEMA }),
      () => agent(labelPrompt(i, 'B'), { label: `label-B:${i}`, phase: 'GoldLabel', schema: LABEL_SCHEMA }),
    ])
    return { i, a, b }
  },
  async ({ i, a, b }) => {
    if (!a || !b) return { i, a, b, final: (a || b)?.labels || [] }
    const mapB = Object.fromEntries(b.labels.map((l) => [l.task, l]))
    // collect agreement pairs and detect disagreement
    const pairs = []
    let disagree = false
    for (const la of a.labels) {
      const lb = mapB[la.task]
      if (!lb) continue
      pairs.push({ field: 'idem', x: la.idempotent, y: lb.idempotent })
      pairs.push({ field: 'pers', x: la.persistent, y: lb.persistent })
      if (la.idempotent !== lb.idempotent || la.persistent !== lb.persistent) disagree = true
    }
    let final
    if (!disagree) {
      final = a.labels.map((l) => ({ task: l.task, idempotent: l.idempotent, persistent: l.persistent }))
    } else {
      const adj = await agent(
        `Two independent annotators labelled tasks in ${a.file}. Reconcile into a FINAL gold label per task, choosing the operationally-correct value (true/false/null) per these criteria.\n${CRITERIA}\n\nAnnotator A: ${JSON.stringify(a.labels)}\nAnnotator B: ${JSON.stringify(b.labels)}\n\nRead ${ROOT}/corpus/asl/${a.file} if you need to inspect a task. Return the final label for every task A or B labelled.`,
        { label: `adjudicate:${i}`, phase: 'Adjudicate', schema: ADJ_SCHEMA },
      )
      final = adj?.final || a.labels.map((l) => ({ task: l.task, idempotent: l.idempotent, persistent: l.persistent }))
    }
    return { i, file: a.file, pairs, final }
  },
)

// ---------- Phase 3+4: precision triage ----------
phase('Triage')
const triageResults = await pipeline(
  Array.from({ length: TRIAGE_N }, (_, i) => i),
  (i) => agent(triagePrompt(i), { label: `triage:${i}`, phase: 'Triage', schema: TRIAGE_SCHEMA }).then((r) => ({ i, r })),
  async ({ i, r }) => {
    if (!r) return { i, verdicts: [] }
    const fps = r.verdicts.filter((v) => v.verdict === 'FALSE_POSITIVE')
    if (fps.length === 0) return { i, verdicts: r.verdicts }
    // adversarially verify the claimed false positives
    const v = await agent(
      `A triager judged these StepCheck warnings as FALSE_POSITIVE. Be skeptical: for each, decide whether it is REALLY a false positive or whether the triager missed a genuine risk (i.e. it is actually a TRUE_POSITIVE). Default to TRUE_POSITIVE when genuinely uncertain.\n${CRITERIA}\n\nRead ${ARGS}, take triage[${i}].f, read ${ROOT}/corpus/asl/<f>, inspect the named tasks. Claimed false positives: ${JSON.stringify(fps)}.\nReturn a final verdict for EACH of these states.`,
      { label: `verify:${i}`, phase: 'Verify', schema: TRIAGE_SCHEMA },
    )
    const override = Object.fromEntries((v?.verdicts || []).map((x) => [x.state + '|' + x.code, x.verdict]))
    const merged = r.verdicts.map((x) => {
      const k = x.state + '|' + x.code
      return k in override ? { ...x, verdict: override[k] } : x
    })
    return { i, verdicts: merged }
  },
)

// ---------- Aggregate: Cohen's kappa + precision ----------
function kappa(pairs) {
  // categories: true/false/null
  const cats = ['true', 'false', 'null']
  const key = (v) => (v === true ? 'true' : v === false ? 'false' : 'null')
  const n = pairs.length
  if (n === 0) return { n: 0, po: 0, pe: 0, kappa: 0 }
  let agree = 0
  const mx = {}, my = {}
  for (const c of cats) { mx[c] = 0; my[c] = 0 }
  for (const p of pairs) {
    const a = key(p.x), b = key(p.y)
    if (a === b) agree++
    mx[a]++; my[b]++
  }
  const po = agree / n
  let pe = 0
  for (const c of cats) pe += (mx[c] / n) * (my[c] / n)
  const k = pe === 1 ? 1 : (po - pe) / (1 - pe)
  return { n, po: +po.toFixed(4), pe: +pe.toFixed(4), kappa: +k.toFixed(4) }
}

const allPairs = goldResults.filter(Boolean).flatMap((g) => g.pairs || [])
const idemPairs = allPairs.filter((p) => p.field === 'idem')
const persPairs = allPairs.filter((p) => p.field === 'pers')

// build expanded gold labels (omit null = abstain), keyed by file
const goldLabels = {}
let goldTaskCount = 0
for (const g of goldResults.filter(Boolean)) {
  if (!g.file || !g.final) continue
  const obj = {}
  for (const l of g.final) {
    if (l.idempotent === null && l.persistent === null) continue // unlabelable
    obj[l.task] = { idempotent: l.idempotent, persistent: l.persistent }
    goldTaskCount++
  }
  if (Object.keys(obj).length) goldLabels[g.file] = obj
}

// precision
const allVerdicts = triageResults.filter(Boolean).flatMap((t) => t.verdicts || [])
function prec(code) {
  const v = code ? allVerdicts.filter((x) => x.code === code) : allVerdicts
  const tp = v.filter((x) => x.verdict === 'TRUE_POSITIVE').length
  const fp = v.filter((x) => x.verdict === 'FALSE_POSITIVE').length
  const tot = tp + fp
  return { total: tot, tp, fp, precision: tot ? +(tp / tot).toFixed(4) : null }
}

log(`gold: ${Object.keys(goldLabels).length} files, ${goldTaskCount} labelled tasks; idem kappa=${kappa(idemPairs).kappa}, pers kappa=${kappa(persPairs).kappa}`)
log(`precision overall=${JSON.stringify(prec(null))}`)

return {
  gold: {
    files: Object.keys(goldLabels).length,
    labelledTasks: goldTaskCount,
    agreement: { idempotency: kappa(idemPairs), persistence: kappa(persPairs) },
    labels: goldLabels,
  },
  precision: {
    overall: prec(null),
    SC3001: prec('SC3001'),
    SC4001: prec('SC4001'),
    verdicts: allVerdicts,
  },
}
