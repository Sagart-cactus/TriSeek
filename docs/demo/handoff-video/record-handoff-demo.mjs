#!/usr/bin/env node
import { createWriteStream, existsSync, mkdirSync, readFileSync, statSync, writeFileSync, copyFileSync } from "node:fs";
import http from "node:http";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import { spawn, spawnSync } from "node:child_process";

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(__dirname, "../../..");
const wtermRoot = process.env.WTERM_ROOT || "/Users/trivedi/Documents/Projects/wterm";
const wtermLocal = join(wtermRoot, "examples/local");
const outDir = resolve(process.env.HANDOFF_VIDEO_OUT || join(__dirname, "output"));
const triseekHome = join(outDir, ".triseek-home");
const wtermPort = Number(process.env.WTERM_DEMO_PORT || 3210);
const wtermUrl = `http://127.0.0.1:${wtermPort}`;
const viewport = { width: 1440, height: 900 };
const sessionId = process.env.HANDOFF_SESSION_ID || "demo-handoff";
const demoEnv = {
  ...process.env,
  TRISEEK_HOME: triseekHome,
  HANDOFF_VIDEO_OUT: outDir,
  TRISEEK_DEMO_HOME: join(outDir, "demo-home"),
  TRISEEK_DEMO_ROOT: repoRoot,
  TERM: "xterm-256color",
  COLORTERM: "truecolor",
};

function log(message) {
  process.stdout.write(`[handoff-video] ${message}\n`);
}

function commandExists(command) {
  return spawnSync("bash", ["-lc", `command -v ${command}`], { stdio: "ignore" }).status === 0;
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: options.cwd || repoRoot,
    env: options.env || demoEnv,
    encoding: "utf-8",
    stdio: options.stdio || "pipe",
    timeout: options.timeout || 120000,
  });
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed\n${result.stdout || ""}${result.stderr || ""}`);
  }
  return result.stdout || "";
}

function preflight() {
  for (const command of ["pnpm", "node", "ffmpeg", "say"]) {
    if (!commandExists(command)) throw new Error(`${command} is required for the demo`);
  }
  if (!existsSync(join(wtermRoot, "node_modules/@playwright/test"))) {
    throw new Error(`Playwright was not found in ${wtermRoot}/node_modules/@playwright/test`);
  }
  mkdirSync(outDir, { recursive: true });
  mkdirSync(triseekHome, { recursive: true });
  log("building latest TriSeek and preparing demo-scoped Claude/Codex MCP config");
  run("bash", [join(__dirname, "install-demo-mcp.sh")], { stdio: "pipe" });
}

function ensureWasm() {
  const wasmTarget = join(wtermLocal, "public/wterm.wasm");
  if (existsSync(wasmTarget)) return;
  const wasmSource = join(wtermRoot, "node_modules/@wterm/core/wasm/wterm.wasm");
  if (!existsSync(wasmSource)) throw new Error(`missing wterm wasm at ${wasmSource}`);
  mkdirSync(dirname(wasmTarget), { recursive: true });
  copyFileSync(wasmSource, wasmTarget);
}

function startWterm() {
  ensureWasm();
  const logPath = join(outDir, "wterm-server.log");
  const logStream = createWriteStream(logPath, { flags: "w" });
  const child = spawn("pnpm", ["--dir", wtermLocal, "exec", "tsx", "server.ts"], {
    cwd: wtermLocal,
    env: {
      ...demoEnv,
      HOST: "127.0.0.1",
      PORT: String(wtermPort),
      NODE_ENV: "development",
    },
    stdio: ["ignore", "pipe", "pipe"],
    detached: true,
  });
  child.stdout.pipe(logStream);
  child.stderr.pipe(logStream);
  return child;
}

function stopProcessTree(child) {
  if (!child || child.killed) return;
  try {
    process.kill(-child.pid, "SIGTERM");
  } catch {
    try {
      child.kill("SIGTERM");
    } catch {
      // Best effort cleanup.
    }
  }
}

function waitForHttp(url, timeoutMs = 45000) {
  const started = Date.now();
  return new Promise((resolveWait, rejectWait) => {
    const tick = () => {
      const req = http.get(url, (res) => {
        res.resume();
        if (res.statusCode && res.statusCode < 500) {
          resolveWait();
        } else if (Date.now() - started > timeoutMs) {
          rejectWait(new Error(`timed out waiting for ${url}`));
        } else {
          setTimeout(tick, 500);
        }
      });
      req.on("error", () => {
        if (Date.now() - started > timeoutMs) rejectWait(new Error(`timed out waiting for ${url}`));
        else setTimeout(tick, 500);
      });
      req.setTimeout(1000, () => req.destroy());
    };
    tick();
  });
}

async function loadPlaywright() {
  const requireFromWterm = createRequire(join(wtermRoot, "package.json"));
  const mod = requireFromWterm("@playwright/test");
  return mod.chromium;
}

async function waitForTerminal(page) {
  await page.waitForSelector('[role="textbox"], .wterm', { timeout: 30000 });
  await page.waitForTimeout(2500);
  await page.mouse.click(viewport.width / 2, viewport.height / 2);
  await page.waitForTimeout(300);
  await page.keyboard.press("Control+L");
  await page.waitForTimeout(400);
}

async function typeCommand(page, command, sentinel, waitMs = 600) {
  const full = `${command}; printf '\\n${sentinel}\\n'\n`;
  await page.keyboard.type(full, { delay: 6 });
  await page.waitForFunction((text) => document.body.innerText.includes(text), sentinel, { timeout: 90000 });
  await page.waitForTimeout(waitMs);
}

async function typeLine(page, line, expectedText, waitMs = 900) {
  await page.keyboard.type(`${line}\n`, { delay: 12 });
  if (expectedText) {
    await page.waitForFunction((text) => document.body.innerText.includes(text), expectedText, { timeout: 120000 });
  }
  await page.waitForTimeout(waitMs);
}

async function installTerminalSurface(page) {
  await page.addStyleTag({
    content: `
      html, body { margin: 0 !important; width: 100%; height: 100%; background: #101010 !important; overflow: hidden; }
      #demo-terminal {
        box-sizing: border-box;
        width: 100vw;
        height: 100vh;
        margin: 0;
        padding: 24px 28px;
        color: #e7e7e7;
        background: #101010;
        font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace;
        font-size: 16px;
        line-height: 1.24;
        white-space: pre-wrap;
      }
      .flex.h-screen { display: none !important; }
    `,
  });
  await page.evaluate(() => {
    document.body.innerHTML = '<pre id="demo-terminal"></pre>';
  });
}

async function setTerminal(page, text, waitMs = 500) {
  await page.evaluate((value) => {
    const el = document.querySelector("#demo-terminal");
    if (el) el.textContent = value;
  }, text);
  await page.waitForTimeout(waitMs);
}

async function typeOnTerminal(page, prefix, command, waitMs = 700) {
  let text = prefix;
  for (const ch of command) {
    text += ch;
    await page.evaluate((value) => {
      const el = document.querySelector("#demo-terminal");
      if (el) el.textContent = value;
    }, text);
    await page.waitForTimeout(16);
  }
  await setTerminal(page, `${text}\n`, waitMs);
}

function claudeScreen({ status, user, assistant, tools = [] }) {
  return [
    "+----------------------------------------------------------------------------------------------------------------------+",
    `| Claude Code  |  /Users/trivedi/Documents/Projects/TriSeek                                                            |`,
    `| TriSeek session: demo-handoff  |  ${status.padEnd(80).slice(0, 80)} |`,
    "+----------------------------------------------------------------------------------------------------------------------+",
    `| User:   ${user.padEnd(107).slice(0, 107)} |`,
    `| Claude: ${assistant.padEnd(107).slice(0, 107)} |`,
    "+----------------------------------------------------------------------------------------------------------------------+",
    "| Tool activity                                                                                                         |",
    ...[0, 1, 2, 3, 4, 5].map((i) => `| ${(tools[i] || "").padEnd(116).slice(0, 116)} |`),
    "+----------------------------------------------------------------------------------------------------------------------+",
    "claude> ",
  ].join("\n");
}

function codexScreen({ status, user, assistant, tools = [] }) {
  return [
    "+----------------------------------------------------------------------------------------------------------------------+",
    `| Codex CLI  |  /Users/trivedi/Documents/Projects/TriSeek                                                              |`,
    `| TriSeek session: demo-handoff  |  ${status.padEnd(80).slice(0, 80)} |`,
    "+----------------------------------------------------------------------------------------------------------------------+",
    `| User:  ${user.padEnd(108).slice(0, 108)} |`,
    `| Codex: ${assistant.padEnd(108).slice(0, 108)} |`,
    "+----------------------------------------------------------------------------------------------------------------------+",
    "| Tool activity                                                                                                         |",
    ...[0, 1, 2, 3, 4, 5].map((i) => `| ${(tools[i] || "").padEnd(116).slice(0, 116)} |`),
    "+----------------------------------------------------------------------------------------------------------------------+",
    "codex> ",
  ].join("\n");
}

function shortId(snapshotId) {
  return snapshotId ? `${snapshotId.slice(0, 8)}...${snapshotId.slice(-6)}` : "";
}

async function runRecording() {
  const chromium = await loadPlaywright();
  const browser = await chromium.launch({ headless: false });
  const context = await browser.newContext({
    viewport,
    recordVideo: { dir: outDir, size: viewport },
  });
  const page = await context.newPage();
  await page.goto(wtermUrl, { waitUntil: "networkidle" });
  await waitForTerminal(page);
  await installTerminalSurface(page);

  await setTerminal(page, "$ ", 500);
  await typeOnTerminal(page, "$ ", "claude", 800);
  await setTerminal(
    page,
    claudeScreen({
      status: "new session",
      user: "Please inspect the portability work and make a small change in this session.",
      assistant: "I will inspect the portability layer and leave a concrete trail in the TriSeek session.",
    }),
    900,
  );

  try {
    run("target/debug/triseek", ["daemon", "stop"], { stdio: "ignore", timeout: 5000 });
  } catch {
    // The demo starts from a clean daemon when possible; no daemon is also fine.
  }
  run("target/debug/triseek", ["daemon", "start", "--idle-timeout", "300", "."], { timeout: 120000 });
  await setTerminal(
    page,
    claudeScreen({
      status: "working context captured",
      user: "Please inspect the portability work and make a small change in this session.",
      assistant: "I found the portability surface and saved notes for the next agent.",
      tools: [
        "OK triseek daemon start",
        "OK session_open: demo-handoff",
        "OK search: session_snapshot_create",
        "OK wrote claude-session-notes.md",
      ],
    }),
    1400,
  );
  run("python3", [join(__dirname, "session_rpc.py"), "open", sessionId, "Demo Claude to Codex handoff"]);
  run("python3", [join(__dirname, "session_rpc.py"), "record-search", sessionId, "session_snapshot_create"]);
  writeFileSync(
    join(outDir, "claude-session-notes.md"),
    "# Claude session notes\n\n- Goal: make TriSeek handoff portable between Claude and Codex.\n- Relevant files: protocol.rs, snapshot.rs, hydrate.rs, CLI resume/brief commands.\n",
  );

  await setTerminal(
    page,
    claudeScreen({
      status: "ready for handoff",
      user: "Create a handoff for Codex.",
      assistant: "I will package the session so Codex can resume with the goal, touched files, and search history.",
      tools: [
        "OK triseek daemon start",
        "OK session_open: demo-handoff",
        "OK search: session_snapshot_create",
        "RUN triseek snapshot create",
      ],
    }),
    900,
  );
  const snapshotJson = run("target/debug/triseek", [
    "snapshot",
    "create",
    "--session",
    sessionId,
    "--source-harness",
    "claude_code",
    "--pin",
    "crates/search-core/src/protocol.rs:320:370",
    "--pin",
    "crates/search-server/src/snapshot.rs:1:160",
  ]);
  writeFileSync(join(outDir, "snapshot.json"), snapshotJson);
  const snapshotId = JSON.parse(snapshotJson).snapshot_id;
  writeFileSync(join(outDir, "current-snapshot-id.txt"), `${snapshotId}\n`);
  const brief = run("target/debug/triseek", ["brief", snapshotId, "--mode", "no-inference"]);
  writeFileSync(join(outDir, "brief.txt"), brief);
  await setTerminal(
    page,
    claudeScreen({
      status: "handoff ready",
      user: "Create a handoff for Codex.",
      assistant: `Handoff ready for Codex. Snapshot: ${shortId(snapshotId)}`,
      tools: [
        "OK triseek daemon start",
        "OK session_open: demo-handoff",
        "OK search: session_snapshot_create",
        `OK snapshot create: ${shortId(snapshotId)}`,
        "OK brief --mode no-inference",
      ],
    }),
    1600,
  );

  await typeOnTerminal(page, `${claudeScreen({
    status: "handoff ready",
    user: "Create a handoff for Codex.",
    assistant: `Handoff ready for Codex. Snapshot: ${shortId(snapshotId)}`,
    tools: [
      "OK triseek daemon start",
      "OK session_open: demo-handoff",
      "OK search: session_snapshot_create",
      `OK snapshot create: ${shortId(snapshotId)}`,
      "OK brief --mode no-inference",
    ],
  })}`, "/quit", 700);
  await setTerminal(page, "$ ", 700);
  await typeOnTerminal(page, "$ ", "codex", 800);
  await setTerminal(
    page,
    codexScreen({
      status: "new session",
      user: "Restore the Claude handoff.",
      assistant: "I will restore Claude's snapshot before doing any new work.",
    }),
    900,
  );

  const resumePath = join(outDir, "resume-AGENTS.md");
  run("target/debug/triseek", ["resume", snapshotId, "--write-to", resumePath]);
  run("target/debug/triseek", ["snapshot", "show", snapshotId]);
  await setTerminal(
    page,
    codexScreen({
      status: "handoff restored",
      user: "Restore the Claude handoff.",
      assistant: "The handoff is loaded. I have the session goal, relevant files, and warm context.",
      tools: [`OK triseek resume: ${shortId(snapshotId)}`, "OK snapshot show", "OK wrote resume-AGENTS.md"],
    }),
    1500,
  );

  const payload = readFileSync(resumePath, "utf-8");
  const payloadPresent = payload.includes("TriSeek Hydration Payload");
  writeFileSync(
    join(outDir, "codex-continuation.md"),
    `# Codex continuation\n\n- Restored snapshot: ${snapshotId}\n- Hydration payload present: ${payloadPresent}\n- Continued from Claude context without rediscovering the repo.\n`,
  );
  await setTerminal(
    page,
    codexScreen({
      status: "continued from context",
      user: "Continue the work from that context.",
      assistant: "Continuation complete. I used Claude's handoff instead of starting cold.",
      tools: [
        `OK triseek resume: ${shortId(snapshotId)}`,
        "OK snapshot show",
        "OK verified TriSeek Hydration Payload",
        "OK wrote codex-continuation.md",
      ],
    }),
    1800,
  );
  await typeOnTerminal(page, `${codexScreen({
    status: "continued from context",
    user: "Continue the work from that context.",
    assistant: "Continuation complete. I used Claude's handoff instead of starting cold.",
    tools: [
      `OK triseek resume: ${shortId(snapshotId)}`,
      "OK snapshot show",
      "OK verified TriSeek Hydration Payload",
      "OK wrote codex-continuation.md",
    ],
  })}`, "/quit", 1200);
  await setTerminal(page, "$ ", 1200);

  await page.screenshot({ path: join(outDir, "final-frame.png"), fullPage: false });
  const video = await page.video().path();
  await context.close();
  await browser.close();
  return video;
}

function convertVideo(rawVideo) {
  const rawTarget = join(outDir, "raw-video.webm");
  copyFileSync(rawVideo, rawTarget);
  const mp4 = join(outDir, "handoff-demo.mp4");
  run("ffmpeg", [
    "-y",
    "-ss",
    "5.6",
    "-i",
    rawTarget,
    "-vf",
    "fps=30,format=yuv420p",
    "-movflags",
    "+faststart",
    mp4,
  ]);
  return mp4;
}

function writeManifest(mp4) {
  const manifest = {
    generated_at: new Date().toISOString(),
    repo_root: repoRoot,
    wterm_url: wtermUrl,
    session_id: sessionId,
    output_video: mp4,
    narration_script: join(__dirname, "narration.txt"),
  };
  writeFileSync(join(outDir, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
}

async function main() {
  preflight();
  log(`starting wterm local PTY at ${wtermUrl}`);
  const server = startWterm();
  let mp4;
  try {
    await waitForHttp(wtermUrl);
    log("recording browser terminal");
    const rawVideo = await runRecording();
    log("converting Playwright video to mp4");
    mp4 = convertVideo(rawVideo);
    const size = statSync(mp4).size;
    if (size < 1024) throw new Error(`final video is too small: ${size} bytes`);
    writeManifest(mp4);
    log(`wrote ${mp4}`);
  } finally {
    stopProcessTree(server);
    try {
      run("target/debug/triseek", ["daemon", "stop"], { stdio: "ignore", timeout: 5000 });
    } catch {
      // The daemon may already be gone; the demo artifacts are already written.
    }
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
