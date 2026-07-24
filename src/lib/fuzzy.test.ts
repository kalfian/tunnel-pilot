import { describe, expect, it } from "vitest";
import { fuzzyMatch, fuzzyRank } from "./fuzzy";

describe("fuzzyMatch", () => {
  it("matches everything on an empty query with score 0", () => {
    const r = fuzzyMatch("", "Postgres (prod)");
    expect(r.matched).toBe(true);
    expect(r.score).toBe(0);
  });

  it("matches a subsequence and reports positions", () => {
    const r = fuzzyMatch("pgprd", "postgres-prod");
    expect(r.matched).toBe(true);
    expect(r.positions.length).toBe(5);
  });

  it("rejects a non-subsequence", () => {
    expect(fuzzyMatch("xyz", "postgres-prod").matched).toBe(false);
  });

  it("is case-insensitive", () => {
    expect(fuzzyMatch("POSTGRES", "postgres").matched).toBe(true);
  });

  it("ranks an exact match above a prefix above a scattered match", () => {
    const exact = fuzzyMatch("redis", "redis").score;
    const prefix = fuzzyMatch("redis", "redis-staging").score;
    const scattered = fuzzyMatch("redis", "r-e-d-i-s-cache").score;
    expect(exact).toBeGreaterThan(prefix);
    expect(prefix).toBeGreaterThan(scattered);
  });

  it("rewards word-boundary starts", () => {
    // 'db' at a word boundary should beat 'db' buried mid-token.
    const boundary = fuzzyMatch("db", "prod db").score;
    const buried = fuzzyMatch("db", "adbc").score;
    expect(boundary).toBeGreaterThan(buried);
  });

  it("supports multi-term AND matching, order-independent across terms", () => {
    const target = "127.0.0.1:5432 postgres prod db";
    expect(fuzzyMatch("prod db", target).matched).toBe(true);
    expect(fuzzyMatch("db prod", target).matched).toBe(true);
    // A term that isn't present fails the whole match.
    expect(fuzzyMatch("prod redis", target).matched).toBe(false);
  });
});

describe("fuzzyRank", () => {
  const items = [
    { name: "Postgres (prod)" },
    { name: "Redis (prod)" },
    { name: "Postgres (staging)" },
  ];

  it("keeps input order for an empty query", () => {
    const ranked = fuzzyRank("", items, (i) => i.name);
    expect(ranked.map((r) => r.item.name)).toEqual([
      "Postgres (prod)",
      "Redis (prod)",
      "Postgres (staging)",
    ]);
  });

  it("drops non-matches and ranks the best match first", () => {
    const ranked = fuzzyRank("redis", items, (i) => i.name);
    expect(ranked).toHaveLength(1);
    expect(ranked[0].item.name).toBe("Redis (prod)");
  });

  it("is stable for equal scores (keeps input order)", () => {
    const dupes = [{ name: "api" }, { name: "api" }, { name: "api" }];
    const ranked = fuzzyRank("api", dupes, (i) => i.name);
    expect(ranked).toHaveLength(3);
  });
});
