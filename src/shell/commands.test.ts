import { describe, expect, it } from "vitest";
import { matchScore, pushRecent, rank, type Command } from "./commands";

const cmd = (id: string, label: string, group: Command["group"] = "Pages", keywords?: string[]): Command => ({ id, label, group, keywords, run: () => {} });

describe("matchScore", () => {
  it("orders exact > prefix > word prefix > substring > in-order letters", () => {
    const exact = matchScore("storage", "Storage");
    const prefix = matchScore("stor", "Storage");
    const word = matchScore("temp", "Clean temporary files");
    const sub = matchScore("orag", "Storage");
    const fuzzy = matchScore("stg", "Storage");
    expect(exact).toBeGreaterThan(prefix);
    expect(prefix).toBeGreaterThan(word);
    expect(word).toBeGreaterThan(sub);
    expect(sub).toBeGreaterThan(fuzzy);
    expect(fuzzy).toBeGreaterThan(0);
  });
  it("matches several words in any order, ignoring case and accents", () => {
    expect(matchScore("files clean", "Clean temporary files")).toBeGreaterThan(0);
    expect(matchScore("cafe", "Café playlist")).toBeGreaterThan(0);
  });
  it("matches initials", () => {
    expect(matchScore("ws", "Windows settings")).toBeGreaterThan(0);
  });
  it("rejects non-matches and single stray letters", () => {
    expect(matchScore("xyz", "Storage")).toBe(0);
    expect(matchScore("q", "Storage")).toBe(0);
  });
});

describe("rank", () => {
  const all = [
    cmd("p:storage", "Storage"),
    cmd("p:startup", "Startup apps"),
    cmd("a:clean", "Clean temporary files", "Actions", ["junk", "storage"]),
    cmd("m:focus", "Focus", "Music"),
  ];

  it("puts the best match's group first and keeps groups together", () => {
    const r = rank("stor", all, []);
    expect(r[0].id).toBe("p:storage");
    const groups = r.map((c) => c.group);
    expect(groups.indexOf("Actions")).toBeGreaterThan(groups.lastIndexOf("Pages"));
  });

  it("finds commands by keyword", () => {
    expect(rank("junk", all, []).map((c) => c.id)).toEqual(["a:clean"]);
  });

  it("nudges recently used commands up", () => {
    const plain = rank("st", all, []);
    const withRecent = rank("st", all, ["p:startup"]);
    expect(withRecent.indexOf(withRecent.find((c) => c.id === "p:startup")!)).toBeLessThanOrEqual(plain.indexOf(plain.find((c) => c.id === "p:startup")!));
    expect(withRecent[0].id).toBe("p:startup");
  });

  it("shows recent then suggestions for an empty query, without duplicates", () => {
    const r = rank("", all, ["m:focus", "gone"], { suggestions: ["m:focus", "p:storage"] });
    expect(r.map((c) => c.id)).toEqual(["m:focus", "p:storage"]);
    expect(r.map((c) => c.group)).toEqual(["Recent", "Suggested"]);
  });

  it("caps each group", () => {
    const many = Array.from({ length: 20 }, (_, i) => cmd(`x${i}`, `Track ${i}`, "Music"));
    expect(rank("track", many, [], { perGroup: 4 })).toHaveLength(4);
  });
});

describe("pushRecent", () => {
  it("moves to front, dedupes and caps at 8", () => {
    expect(pushRecent(["a", "b", "c"], "b")).toEqual(["b", "a", "c"]);
    expect(pushRecent(["1", "2", "3", "4", "5", "6", "7", "8"], "9")).toHaveLength(8);
  });
});
