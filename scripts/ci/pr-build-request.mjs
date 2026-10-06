import { appendFileSync, readFileSync } from "node:fs";

const eventPath = process.env.GITHUB_EVENT_PATH;
if (!eventPath) {
  console.error("Missing GITHUB_EVENT_PATH");
  process.exit(1);
}

const event = JSON.parse(readFileSync(eventPath, "utf8"));
const body = (event.comment?.body || "").trim();
const command = body.match(/^\/build(?:\s+([a-z0-9-]+))?\s*$/i);

if (!command) {
  setOutput("should_build", "false");
  process.exit(0);
}

const issueNumber = event.issue.number;
const username = event.comment.user.login;
const repo = process.env.GITHUB_REPOSITORY;
const [owner, repoName] = repo.split("/");
const serverUrl = process.env.GITHUB_SERVER_URL || "https://github.com";
const runId = process.env.GITHUB_RUN_ID;
const commentId = event.comment.id;

async function ghApi(endpoint, options = {}) {
  const method = options.method || "GET";
  const url = `https://api.github.com/repos/${owner}/${repoName}/${endpoint.replace(/^\//, "")}`;
  const headers = {
    Accept: "application/vnd.github.v3+json",
    Authorization: `Bearer ${process.env.GH_TOKEN}`,
    "User-Agent": "neoOMSI-pr-build-ci",
    ...(options.headers || {}),
  };
  const bodyData = options.body ? JSON.stringify(options.body) : undefined;
  const res = await fetch(url, { method, headers, body: bodyData });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(
      `GitHub API ${method} ${endpoint} returned ${res.status}: ${text}`,
    );
  }
  if (res.status === 204) return null;
  return res.json();
}

async function postComment(text) {
  return ghApi(`issues/${issueNumber}/comments`, {
    method: "POST",
    body: { body: text },
  });
}

function setOutput(key, value) {
  const outputFile = process.env.GITHUB_OUTPUT;
  if (outputFile) {
    appendFileSync(outputFile, `${key}=${value}\n`);
  } else {
    console.log(`OUTPUT: ${key}=${value}`);
  }
}

async function main() {
  let permission = "none";
  try {
    const permData = await ghApi(
      `collaborators/${encodeURIComponent(username)}/permission`,
    );
    permission = permData.permission;
  } catch (err) {
    console.info(
      `Could not resolve repository permission for ${username}: ${err.message}`,
    );
  }

  if (!["write", "maintain", "admin"].includes(permission)) {
    await postComment(
      "PR builds can only be requested by repository members with write, maintain, or admin permission.",
    );
    setOutput("should_build", "false");
    return;
  }

  const target = (command[1] || "windows-x64").toLowerCase();
  const targets = {
    "windows-x64": {
      label: "Windows x64",
      platform: "windows",
      arch: "x64",
      runner: "windows-latest",
      target: "x86_64-pc-windows-msvc",
    },
    "windows-arm64": {
      label: "Windows ARM64",
      platform: "windows",
      arch: "arm64",
      runner: "windows-latest",
      target: "aarch64-pc-windows-msvc",
    },
    "linux-x64": {
      label: "Linux x64",
      platform: "linux",
      arch: "x64",
      runner: "ubuntu-22.04",
      target: "",
    },
    "linux-arm64": {
      label: "Linux ARM64",
      platform: "linux",
      arch: "arm64",
      runner: "ubuntu-22.04-arm",
      target: "",
    },
    "macos-x64": {
      label: "macOS x64",
      platform: "macos",
      arch: "x64",
      runner: "macos-latest",
      target: "x86_64-apple-darwin",
    },
    "macos-arm64": {
      label: "macOS ARM64",
      platform: "macos",
      arch: "arm64",
      runner: "macos-latest",
      target: "aarch64-apple-darwin",
    },
  };

  const aliases = {
    windows: "windows-x64",
    win: "windows-x64",
    "win-x64": "windows-x64",
    "win-arm64": "windows-arm64",
    linux: "linux-x64",
    macos: "macos-arm64",
    mac: "macos-arm64",
  };

  const resolvedTarget = aliases[target] || target;
  let selected;

  if (resolvedTarget === "all") {
    selected = Object.values(targets);
  } else if (targets[resolvedTarget]) {
    selected = [targets[resolvedTarget]];
  } else {
    await postComment(
      [
        `Unknown build target \`${target}\`.`,
        "",
        "Available commands:",
        "",
        "```text",
        "/build",
        "/build windows-x64",
        "/build windows-arm64",
        "/build linux-x64",
        "/build linux-arm64",
        "/build macos-x64",
        "/build macos-arm64",
        "/build all",
        "```",
      ].join("\n"),
    );
    setOutput("should_build", "false");
    return;
  }

  const pull = await ghApi(`pulls/${issueNumber}`);
  if (pull.state !== "open") {
    await postComment(
      "PR builds can only be requested for open pull requests.",
    );
    setOutput("should_build", "false");
    return;
  }

  const headRepository = pull.head?.repo?.full_name;
  const headSha = pull.head?.sha;

  if (!headRepository || !headSha) {
    await postComment(
      "The pull request source repository is no longer available, so it cannot be built.",
    );
    setOutput("should_build", "false");
    return;
  }

  const versionFile = await ghApi("contents/VERSION");
  if (!versionFile || versionFile.type !== "file") {
    throw new Error("VERSION is not a file");
  }

  const baseVersion = Buffer.from(versionFile.content, "base64")
    .toString("utf8")
    .trim();
  const shortSha = headSha.slice(0, 8);
  const version = `${baseVersion}-pr.${issueNumber}.g${shortSha}`;
  const requestLabel =
    selected.length === Object.keys(targets).length
      ? "all targets"
      : selected[0].label;
  const runUrl = `${serverUrl}/${owner}/${repoName}/actions/runs/${runId}`;

  // Add reaction
  try {
    await ghApi(`issues/comments/${commentId}/reactions`, {
      method: "POST",
      body: { content: "eyes" },
    });
  } catch (err) {
    console.warn(`Could not add reaction: ${err.message}`);
  }

  const status = await postComment(
    [
      "### PR build requested",
      "",
      `Building **${requestLabel}** from commit \`${shortSha}\`.`,
      "",
      `[Open workflow run](${runUrl})`,
    ].join("\n"),
  );

  setOutput("should_build", "true");
  setOutput("matrix", JSON.stringify({ include: selected }));
  setOutput("source_repository", headRepository);
  setOutput("source_ref", headSha);
  setOutput("version", version);
  setOutput("short_sha", shortSha);
  setOutput("request_label", requestLabel);
  setOutput("status_comment_id", String(status.id));
}

main().catch(async (err) => {
  console.error(err);
  process.exit(1);
});
