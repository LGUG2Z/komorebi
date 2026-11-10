#!/usr/bin/env node

const fs = require("fs");
const path = require("path");

const cliBaseDir = path.join(__dirname, "../src/data/cli");
const windowsDir = path.join(cliBaseDir, "windows");
const macosDir = path.join(cliBaseDir, "macos");
const windowsOutputFile = path.join(
  __dirname,
  "../src/content/docs/reference/komorebic-windows.mdx",
);
const macosOutputFile = path.join(
  __dirname,
  "../src/content/docs/reference/komorebic-macos.mdx",
);

// Helper to read markdown files from a directory
function readMarkdownFiles(dir) {
  if (!fs.existsSync(dir)) {
    return [];
  }
  return fs
    .readdirSync(dir)
    .filter((f) => f.endsWith(".md"))
    .sort()
    .map((file) => ({
      name: file.replace(".md", ""),
      content: fs.readFileSync(path.join(dir, file), "utf-8"),
    }));
}

// Generate Windows reference
const windowsFiles = readMarkdownFiles(windowsDir);
if (windowsFiles.length > 0) {
  let mdx = `---
title: komorebic (Windows)
description: Complete reference for all komorebic commands on Windows.
---

This page contains a complete reference of all \`komorebic\` commands on Windows.

`;
  for (const { name, content } of windowsFiles) {
    mdx += `### \`${name}\`\n\n`;
    mdx += content + "\n\n";
  }
  fs.writeFileSync(windowsOutputFile, mdx);
  console.log(
    `Generated ${windowsOutputFile} with ${windowsFiles.length} commands`,
  );
}

// Generate macOS reference
const macosFiles = readMarkdownFiles(macosDir);
if (macosFiles.length > 0) {
  let mdx = `---
title: komorebic (macOS)
description: Complete reference for all komorebic commands on macOS.
---

This page contains a complete reference of all \`komorebic\` commands on macOS.

`;
  for (const { name, content } of macosFiles) {
    mdx += `### \`${name}\`\n\n`;
    mdx += content + "\n\n";
  }
  fs.writeFileSync(macosOutputFile, mdx);
  console.log(
    `Generated ${macosOutputFile} with ${macosFiles.length} commands`,
  );
}

if (windowsFiles.length === 0 && macosFiles.length === 0) {
  console.log("No CLI documentation found in windows or macos directories");
}
