#!/usr/bin/env node
// Checagem das traduções (doc 15 §8, etapa 7). Falha o CI quando:
//  1. pt-BR e en-US não têm exatamente as mesmas chaves;
//  2. o código usa t("chave") que não existe;
//  3. existe chave que nenhum código usa (órfã).
//
// Uma chave conta como usada quando aparece como string literal completa no
// código (src/ ou src-tauri/src, que manda chaves de mensagem), ou quando fica
// debaixo de um prefixo montado em template literal (`controls.d3.sections.${x}.title`
// conta para tudo em "controls.d3.sections.").
//
// Uso: node scripts/check-i18n.mjs [--list-unused]
import { build } from "esbuild";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { createRequire } from "node:module";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const listUnused = process.argv.includes("--list-unused");

const outDir = mkdtempSync(path.join(tmpdir(), "ab-i18n-"));
const outFile = path.join(outDir, "translations.cjs");
await build({
  entryPoints: [path.join(root, "src/i18n/translations.ts")],
  bundle: true,
  format: "cjs",
  platform: "node",
  outfile: outFile,
  alias: { "@": path.join(root, "src") },
  logLevel: "error",
});
const { translations } = createRequire(import.meta.url)(outFile);
rmSync(outDir, { recursive: true, force: true });

function leaves(object, prefix = "", out = new Map()) {
  for (const [key, value] of Object.entries(object)) {
    const full = prefix ? `${prefix}.${key}` : key;
    if (value && typeof value === "object") leaves(value, full, out);
    else out.set(full, value);
  }
  return out;
}

const pt = leaves(translations["pt-BR"]);
const en = leaves(translations["en-US"]);

function walk(dir, exts, out = []) {
  for (const name of readdirSync(dir)) {
    if (name === "node_modules" || name === "target" || name === "dist") continue;
    const full = path.join(dir, name);
    if (statSync(full).isDirectory()) walk(full, exts, out);
    else if (exts.some((ext) => name.endsWith(ext))) out.push(full);
  }
  return out;
}

const i18nDir = path.join(root, "src", "i18n");
const codeFiles = [
  ...walk(path.join(root, "src"), [".ts", ".tsx"]).filter((file) => !file.startsWith(i18nDir)),
  ...walk(path.join(root, "src-tauri", "src"), [".rs"]),
];

const literals = new Set();
const prefixes = new Set();
const calls = new Map();
for (const file of codeFiles) {
  const source = readFileSync(file, "utf8");
  for (const match of source.matchAll(/["'`]([a-zA-Z][a-zA-Z0-9_]*(?:\.[a-zA-Z0-9_]+)+)["'`]/g)) literals.add(match[1]);
  for (const match of source.matchAll(/`([a-zA-Z][a-zA-Z0-9_.]*\.)\$\{/g)) prefixes.add(match[1]);
  if (!file.endsWith(".rs")) {
    for (const match of source.matchAll(/\bt\(\s*["']([a-zA-Z0-9_.]+)["']/g)) {
      calls.set(match[1], path.relative(root, file));
    }
  }
}

const problems = [];

const onlyPt = [...pt.keys()].filter((key) => !en.has(key));
const onlyEn = [...en.keys()].filter((key) => !pt.has(key));
if (onlyPt.length) problems.push(`Só em pt-BR (${onlyPt.length}): ${onlyPt.join(", ")}`);
if (onlyEn.length) problems.push(`Só em en-US (${onlyEn.length}): ${onlyEn.join(", ")}`);

const missing = [...calls.entries()].filter(([key]) => !pt.has(key) || !en.has(key));
if (missing.length) problems.push(`t() com chave inexistente (${missing.length}): ${missing.map(([key, file]) => `${key} (${file})`).join(", ")}`);

const isUsed = (key) => literals.has(key) || [...prefixes].some((prefix) => key.startsWith(prefix));
const unused = [...pt.keys()].filter((key) => !isUsed(key));
if (unused.length) {
  problems.push(`Chaves órfãs, que nenhum código usa (${unused.length})${listUnused ? `: ${unused.join(", ")}` : ". Rode com --list-unused para ver."}`);
}

if (problems.length) {
  console.error("Traduções com problema:\n- " + problems.join("\n- "));
  process.exit(1);
}
console.log(`Traduções ok: ${pt.size} chaves, iguais em pt-BR e en-US, todas usadas.`);
