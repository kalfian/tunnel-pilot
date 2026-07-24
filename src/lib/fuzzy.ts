/**
 * Pure, testable fuzzy matcher for the command palette (spec 05 §10). Kept
 * side-effect-free and dependency-free so the ranking behaviour can be unit
 * tested in isolation and reused for any launcher-style search.
 *
 * Matching model:
 *   - Case-insensitive subsequence match.
 *   - The query is split on whitespace into terms; EVERY term must match
 *     (AND semantics, like fzf) — so "prod db" matches a target containing
 *     both "prod" and "db" in order-independent terms.
 *   - Score rewards prefix / word-boundary / camelCase / consecutive hits and
 *     lightly penalises longer targets, so exact and prefix matches float up.
 *   - Empty query matches everything with score 0 (callers show recents / all).
 */

export interface FuzzyResult {
  /** Whether every query term is a subsequence of the target. */
  matched: boolean;
  /** Higher is better. 0 for an empty query or a non-match. */
  score: number;
  /** Matched character indices in the target (for optional highlighting). */
  positions: number[];
}

const NO_MATCH: FuzzyResult = { matched: false, score: 0, positions: [] };

const BOUNDARY = /[\s\-_/:.()[\]@]/;

function isBoundary(ch: string): boolean {
  return BOUNDARY.test(ch);
}

function isCamelBoundary(original: string, i: number): boolean {
  if (i <= 0) return false;
  const prev = original[i - 1];
  const cur = original[i];
  return prev === prev.toLowerCase() && cur === cur.toUpperCase() && cur !== prev;
}

/** Position bonus for matching a target char at index j (independent of run). */
function charBonus(
  j: number,
  targetLower: string,
  targetOriginal: string,
): number {
  if (j === 0) return 1 + 8;
  if (isBoundary(targetLower[j - 1])) return 1 + 6;
  if (isCamelBoundary(targetOriginal, j)) return 1 + 4;
  return 1;
}

const CONSECUTIVE = 5;
const NEG = Number.NEGATIVE_INFINITY;

/**
 * Score a single already-lowercased term against a target with a small DP so
 * the alignment is optimal (a greedy first-match strands better word-boundary
 * hits — e.g. "db" should align to the "db" word, not the "d" inside "prod").
 */
function matchTerm(
  term: string,
  targetLower: string,
  targetOriginal: string,
): FuzzyResult {
  const n = term.length;
  const m = targetLower.length;
  if (n === 0) return { matched: true, score: 0, positions: [] };
  if (n > m) return NO_MATCH;

  const score: number[][] = Array.from({ length: n }, () =>
    new Array<number>(m).fill(NEG),
  );
  const prev: number[][] = Array.from({ length: n }, () =>
    new Array<number>(m).fill(-1),
  );

  for (let j = 0; j < m; j++) {
    if (targetLower[j] === term[0]) {
      score[0][j] = charBonus(j, targetLower, targetOriginal);
    }
  }

  for (let i = 1; i < n; i++) {
    for (let j = i; j < m; j++) {
      if (targetLower[j] !== term[i]) continue;
      let best = NEG;
      let bestK = -1;
      for (let k = i - 1; k < j; k++) {
        if (score[i - 1][k] === NEG) continue;
        let s = score[i - 1][k] + charBonus(j, targetLower, targetOriginal);
        if (j === k + 1) s += CONSECUTIVE;
        else s -= Math.min(3, (j - k - 1) * 0.5);
        if (s > best) {
          best = s;
          bestK = k;
        }
      }
      score[i][j] = best;
      prev[i][j] = bestK;
    }
  }

  let best = NEG;
  let endJ = -1;
  for (let j = n - 1; j < m; j++) {
    if (score[n - 1][j] > best) {
      best = score[n - 1][j];
      endJ = j;
    }
  }
  if (endJ === -1) return NO_MATCH;

  const positions: number[] = [];
  let i = n - 1;
  let j = endJ;
  while (i >= 0 && j >= 0) {
    positions.push(j);
    j = prev[i][j];
    i--;
  }
  positions.reverse();

  return { matched: true, score: best, positions };
}

/**
 * Fuzzy-match `query` (possibly multi-term) against `target`. Returns whether
 * all terms matched, an aggregate score, and the union of matched positions.
 */
export function fuzzyMatch(query: string, target: string): FuzzyResult {
  const q = query.trim().toLowerCase();
  if (q === "") return { matched: true, score: 0, positions: [] };

  const targetLower = target.toLowerCase();
  const terms = q.split(/\s+/).filter((t) => t.length > 0);

  let total = 0;
  const positions = new Set<number>();
  for (const term of terms) {
    const r = matchTerm(term, targetLower, target);
    if (!r.matched) return NO_MATCH;
    total += r.score;
    for (const p of r.positions) positions.add(p);
  }

  // Whole-string wins big; prefix next. Prefer shorter targets on ties.
  if (targetLower === q) total += 40;
  else if (targetLower.startsWith(q)) total += 20;
  total -= Math.max(0, target.length - q.length) * 0.1;

  return {
    matched: true,
    score: total,
    positions: [...positions].sort((a, b) => a - b),
  };
}

/**
 * Filter + rank a list by fuzzy score against a per-item search string. Stable
 * for equal scores (keeps input order). Non-matches are dropped when the query
 * is non-empty; an empty query keeps every item (score 0) in input order.
 */
export function fuzzyRank<T>(
  query: string,
  items: readonly T[],
  key: (item: T) => string,
): { item: T; result: FuzzyResult }[] {
  const scored = items.map((item, index) => ({
    item,
    index,
    result: fuzzyMatch(query, key(item)),
  }));
  const kept =
    query.trim() === "" ? scored : scored.filter((s) => s.result.matched);
  kept.sort((a, b) =>
    b.result.score !== a.result.score
      ? b.result.score - a.result.score
      : a.index - b.index,
  );
  return kept.map(({ item, result }) => ({ item, result }));
}
