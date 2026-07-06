// Emit source imports as JSON: [{specifier, file, line}, ...]
// Uses the project's TypeScript compiler API when available, else a regex fallback.
// Usage: node parse_ts.js <root>
const fs = require("fs");
const path = require("path");

const root = process.argv[2];
const IGNORE = new Set([
  ".git", "node_modules", "dist", "build", ".next", "out", "coverage",
  ".turbo", ".cache", ".svelte-kit",
]);
const EXTS = new Set([".js", ".jsx", ".ts", ".tsx", ".mjs", ".cjs", ".mts", ".cts"]);

let ts = null;
try {
  ts = require(path.join(root, "node_modules", "typescript"));
} catch (_) {}

const results = [];
const add = (spec, file, line) => {
  if (spec) results.push({ specifier: spec, file: path.relative(root, file), line });
};

function walk(dir) {
  let entries;
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch (_) {
    return;
  }
  for (const e of entries) {
    if (e.name.startsWith(".")) continue;
    const full = path.join(dir, e.name);
    if (e.isDirectory()) {
      if (!IGNORE.has(e.name)) walk(full);
    } else if (EXTS.has(path.extname(e.name))) {
      scan(full);
    }
  }
}

function scan(file) {
  let src;
  try {
    src = fs.readFileSync(file, "utf8");
  } catch (_) {
    return;
  }

  if (ts) {
    try {
      const sf = ts.createSourceFile(file, src, ts.ScriptTarget.Latest, true);
      const lineOf = (node) =>
        sf.getLineAndCharacterOfPosition(node.getStart(sf)).line + 1;
      const visit = (node) => {
        if (
          (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) &&
          node.moduleSpecifier &&
          ts.isStringLiteral(node.moduleSpecifier)
        ) {
          add(node.moduleSpecifier.text, file, lineOf(node));
        } else if (ts.isCallExpression(node)) {
          const e = node.expression;
          const isRequire = ts.isIdentifier(e) && e.text === "require";
          const isDynImport = e.kind === ts.SyntaxKind.ImportKeyword;
          if (
            (isRequire || isDynImport) &&
            node.arguments.length &&
            ts.isStringLiteral(node.arguments[0])
          ) {
            add(node.arguments[0].text, file, lineOf(node));
          }
        }
        ts.forEachChild(node, visit);
      };
      visit(sf);
      return;
    } catch (_) {
      // fall through to regex
    }
  }

  const re =
    /(?:import\s+(?:[^'"]*?\sfrom\s+)?|export\s+[^'"]*?\sfrom\s+|require\s*\(\s*|import\s*\(\s*)['"]([^'"]+)['"]/g;
  let m;
  while ((m = re.exec(src))) {
    const line = src.slice(0, m.index).split("\n").length;
    add(m[1], file, line);
  }
}

walk(root);
process.stdout.write(JSON.stringify(results));
