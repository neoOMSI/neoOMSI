#!/usr/bin/env node
const commentId = Number(process.env.STATUS_COMMENT_ID);
const repo = process.env.GITHUB_REPOSITORY;
const [owner, repoName] = repo.split("/");
const serverUrl = process.env.GITHUB_SERVER_URL || "https://github.com";
const runId = process.env.GITHUB_RUN_ID;
const version = process.env.VERSION;
const shortSha = process.env.SHORT_SHA;
const requestLabel = process.env.REQUEST_LABEL;
const buildResult = process.env.BUILD_RESULT;

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
  return res.json();
}

async function main() {
  const runUrl = `${serverUrl}/${owner}/${repoName}/actions/runs/${runId}`;
  let artifacts = [];
  try {
    const response = await ghApi(
      `actions/runs/${runId}/artifacts?per_page=100`,
    );
    const prefix = `neoOMSI-${version}-`;
    artifacts = (response.artifacts || []).filter(
      (artifact) =>
        !artifact.expired &&
        artifact.name.startsWith(prefix) &&
        !artifact.name.includes("-server-"),
    );
  } catch (err) {
    console.warn(`Could not list workflow artifacts: ${err.message}`);
  }

  const targetOrder = [
    ["-windows-x64.zip", "Windows x64"],
    ["-windows-arm64.zip", "Windows ARM64"],
    ["-linux-x64.zip", "Linux x64"],
    ["-linux-arm64.zip", "Linux ARM64"],
    ["-macos-x64.zip", "macOS x64"],
    ["-macos-arm64.zip", "macOS ARM64"],
  ];

  const rows = targetOrder.flatMap(([suffix, label]) => {
    const artifact = artifacts.find((item) => item.name.endsWith(suffix));
    if (!artifact) return [];
    const url = `${serverUrl}/${owner}/${repoName}/actions/runs/${runId}/artifacts/${artifact.id}`;
    return [`| ${label} | [Download](${url}) |`];
  });

  let body;

  if (buildResult !== "success") {
    if (rows.length > 0) {
      body = [
        "### PR build partially failed",
        "",
        `Commit: \`${shortSha}\``,
        `Target: **${requestLabel}**`,
        "",
        "Some targets failed to build. Available builds from successful targets:",
        "",
        "| Target | Build |",
        "| --- | --- |",
        ...rows,
        "",
        "Artifacts expire after 1 day.",
        "",
        `[Open workflow run](${runUrl})`,
      ].join("\n");
    } else {
      body = [
        "### PR build failed",
        "",
        `Commit: \`${shortSha}\``,
        `Target: **${requestLabel}**`,
        "",
        `[Open workflow run](${runUrl})`,
      ].join("\n");
    }
  } else if (rows.length === 0) {
    body = [
      "### PR build finished, but no downloadable artifact was found",
      "",
      `Commit: \`${shortSha}\``,
      "",
      `[Open workflow run](${runUrl})`,
    ].join("\n");
  } else {
    body = [
      "### PR build ready",
      "",
      `Commit: \`${shortSha}\``,
      `Version: \`${version}\``,
      "",
      "| Target | Build |",
      "| --- | --- |",
      ...rows,
      "",
      "Artifacts expire after 1 day.",
      "",
      `[Open workflow run](${runUrl})`,
    ].join("\n");
  }

  await ghApi(`issues/comments/${commentId}`, {
    method: "PATCH",
    body: { body },
  });
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
