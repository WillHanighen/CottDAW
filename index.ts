import { readFileSync, existsSync } from "fs";
import { join } from "path";

const PROJECT_ROOT = import.meta.dir;
const SRC_DIR = join(PROJECT_ROOT, "src");
const PUBLIC_DIR = join(PROJECT_ROOT, "public");

// Build the main.tsx file on startup
const buildResult = await Bun.build({
  entrypoints: [join(SRC_DIR, "main.tsx")],
  outdir: join(PROJECT_ROOT, ".build"),
  target: "browser",
  format: "esm",
  splitting: false,
  minify: false,
  sourcemap: "external",
});

if (!buildResult.success) {
  console.error("Build failed:");
  for (const log of buildResult.logs) {
    console.error(log);
  }
  process.exit(1);
}

console.log("✓ Built successfully");

const MIME_TYPES: Record<string, string> = {
  ".html": "text/html",
  ".css": "text/css",
  ".js": "application/javascript",
  ".json": "application/json",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".svg": "image/svg+xml",
  ".woff": "font/woff",
  ".woff2": "font/woff2",
  ".ttf": "font/ttf",
  ".wav": "audio/wav",
  ".mp3": "audio/mpeg",
};

function getMimeType(path: string): string {
  const ext = path.substring(path.lastIndexOf("."));
  return MIME_TYPES[ext] || "application/octet-stream";
}

const server = Bun.serve({
  port: 3000,
  async fetch(req) {
    const url = new URL(req.url);
    let pathname = url.pathname;

    // Serve index.html for root
    if (pathname === "/") {
      pathname = "/index.html";
    }

    // Try to serve from .build directory (compiled JS)
    if (pathname === "/main.js") {
      const buildPath = join(PROJECT_ROOT, ".build", "main.js");
      if (existsSync(buildPath)) {
        const content = readFileSync(buildPath);
        return new Response(content, {
          headers: { "Content-Type": "application/javascript" },
        });
      }
    }

    // Try to serve from src directory
    const srcPath = join(SRC_DIR, pathname);
    if (existsSync(srcPath)) {
      const content = readFileSync(srcPath);
      return new Response(content, {
        headers: { "Content-Type": getMimeType(pathname) },
      });
    }

    // Try to serve from public directory
    const publicPath = join(PUBLIC_DIR, pathname);
    if (existsSync(publicPath)) {
      const content = readFileSync(publicPath);
      return new Response(content, {
        headers: { "Content-Type": getMimeType(pathname) },
      });
    }

    // 404
    return new Response("Not Found", { status: 404 });
  },
});

console.log(`🎹 CottDAW running at http://localhost:${server.port}`);
