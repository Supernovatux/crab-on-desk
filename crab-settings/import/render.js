// crab-on-desk, a Rust based desktop pet for coding agents.
//     Copyright (C) 2026  Supernovatux thulashitharan.d@gmail.com
//
//     This program is free software: you can redistribute it and/or modify
//     it under the terms of the GNU Affero General Public License as
//     published by the Free Software Foundation, either version 3 of the
//     License, or (at your option) any later version.
//
//     This program is distributed in the hope that it will be useful,
//     but WITHOUT ANY WARRANTY; without even the implied warranty of
//     MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//     GNU Affero General Public License for more details.
//
//     You should have received a copy of the GNU Affero General Public License
//     along with this program.  If not, see <https://www.gnu.org/licenses/>.

"use strict";

const fs = require("node:fs");
const path = require("node:path");
const zlib = require("node:zlib");
const { pathToFileURL } = require("node:url");
const { app, BrowserWindow } = require("electron");

const USAGE = "usage: electron render.js <reference-repo> <output-themes-dir> <size> <theme>...";
const REACTION_ROLES = {
  drag: "react-drag",
  clickLeft: "react-left",
  clickRight: "react-right",
  annoyed: "react-annoyed",
};
const SLEEP_DEFAULT_MS = {
  mouseIdleTimeout: 20000,
  mouseSleepTimeout: 60000,
  deepSleepTimeout: 600000,
  yawnDuration: 3000,
  wakeDuration: 1500,
};
const DEFAULT_MINI_OFFSET_RATIO = 0.486;
const REACTION_DEFAULT_MS = { clickLeft: 2500, clickRight: 2500, annoyed: 3500, double: 3500 };
const DEFAULT_OBJECT_SCALE = { widthRatio: 1.9, heightRatio: 1.3, offsetX: -0.45, offsetY: -0.25 };
const DEFAULT_LAYOUT = { centerXRatio: 0.5, visibleHeightRatio: 0.58, baselineBottomRatio: 0.05 };
const PNG_SIGNATURE = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

app.commandLine.appendSwitch("force-device-scale-factor", "1");
app.disableHardwareAcceleration();

function roles(theme) {
  const entries = [
    ...Object.entries(theme.states || {}),
    ...Object.entries((theme.miniMode && theme.miniMode.states) || {}),
  ].map(([role, files]) => [role, files[0]]);
  for (const [reaction, role] of Object.entries(REACTION_ROLES)) {
    const entry = theme.reactions && theme.reactions[reaction];
    if (entry && entry.file) entries.push([role, entry.file]);
  }
  return entries;
}

function plan(theme) {
  const clips = roles(theme);
  const roleOfFile = new Map();
  for (const [role, file] of clips) {
    if (!role.startsWith("mini-") && !roleOfFile.has(file)) roleOfFile.set(file, role);
  }
  const role = (prefix, index, file) => {
    if (!roleOfFile.has(file)) {
      const slot = `${prefix}-${index + 1}`;
      clips.push([slot, file]);
      roleOfFile.set(file, slot);
    }
    return roleOfFile.get(file);
  };
  const tiers = (list, prefix) =>
    [...(list || [])]
      .sort((a, b) => a.minSessions - b.minSessions)
      .map((tier, index) => ({ min_sessions: tier.minSessions, animation: role(prefix, index, tier.file) }));
  const double = theme.reactions && theme.reactions.double;
  const timings = theme.timings || {};
  const behaviour = {
    idle_pool: (theme.idleAnimations || []).map((entry, index) => ({
      animation: role("idle-pool", index, entry.file),
      duration_ms: entry.duration,
    })),
    react_double: double ? (double.files || [double.file]).map((file, index) => role("react-double", index, file)) : [],
    working_tiers: tiers(theme.workingTiers, "working-tier"),
    juggling_tiers: tiers(theme.jugglingTiers, "juggling-tier"),
    min_display_ms: { ...timings.minDisplay, ...miniTimings(theme).minDisplay },
    auto_return_ms: { ...timings.autoReturn, ...miniTimings(theme).autoReturn },
    sleep: sleepTimings(timings),
    mini: miniSettings(theme),
    roam_flip_assets: !!theme.roamFlipAssets,
  };
  behaviour.reaction_ms = reactionDurations(theme, behaviour.react_double, double);
  return { clips, behaviour };
}

function miniTimings(theme) {
  return (theme.miniMode && theme.miniMode.timings) || {};
}

function miniSettings(theme) {
  const mini = theme.miniMode;
  if (!mini || mini.supported === false || !mini.states) return null;
  return { offset_ratio: mini.offsetRatio ?? DEFAULT_MINI_OFFSET_RATIO, flip_assets: !!mini.flipAssets };
}

function sleepTimings(timings) {
  const value = (key) => timings[key] ?? SLEEP_DEFAULT_MS[key];
  const sleep = {
    idle_after_ms: value("mouseIdleTimeout"),
    yawn_after_ms: value("mouseSleepTimeout"),
    deep_sleep_after_ms: value("deepSleepTimeout"),
    yawn_ms: value("yawnDuration"),
    wake_ms: value("wakeDuration"),
  };
  if (timings.collapseDuration) sleep.collapse_ms = timings.collapseDuration;
  return sleep;
}

function reactionDurations(theme, doubleRoles, double) {
  const durations = {};
  for (const [reaction, role] of Object.entries(REACTION_ROLES)) {
    const entry = theme.reactions && theme.reactions[reaction];
    if (entry && entry.file && REACTION_DEFAULT_MS[reaction]) durations[role] = entry.duration || REACTION_DEFAULT_MS[reaction];
  }
  for (const role of doubleRoles) durations[role] = double.duration || REACTION_DEFAULT_MS.double;
  return durations;
}

function behaviourToml(behaviour) {
  const string = (value) => JSON.stringify(value);
  const lines = [
    `react_double = [${behaviour.react_double.map(string).join(", ")}]`,
    `roam_flip_assets = ${behaviour.roam_flip_assets}`,
  ];
  for (const idle of behaviour.idle_pool) {
    lines.push("", "[[idle_pool]]", `animation = ${string(idle.animation)}`, `duration_ms = ${idle.duration_ms}`);
  }
  for (const key of ["working_tiers", "juggling_tiers"]) {
    for (const tier of behaviour[key]) {
      lines.push("", `[[${key}]]`, `min_sessions = ${tier.min_sessions}`, `animation = ${string(tier.animation)}`);
    }
  }
  if (behaviour.mini) {
    lines.push("", "[mini]", `offset_ratio = ${behaviour.mini.offset_ratio}`, `flip_assets = ${behaviour.mini.flip_assets}`);
  }
  lines.push("", "[sleep]");
  for (const [key, ms] of Object.entries(behaviour.sleep)) lines.push(`${key} = ${ms}`);
  for (const key of ["min_display_ms", "auto_return_ms", "reaction_ms"]) {
    lines.push("", `[${key}]`);
    for (const [role, ms] of Object.entries(behaviour[key])) lines.push(`${string(role)} = ${ms}`);
  }
  return `${lines.join("\n")}\n`;
}

function sourceFile(reference, themeName, file) {
  const themed = path.join(reference, "themes", themeName, "assets", file);
  return fs.existsSync(themed) ? themed : path.join(reference, "assets", "svg", file);
}

function objectScale(theme) {
  const os = { ...DEFAULT_OBJECT_SCALE, ...(theme.objectScale || {}) };
  const viewBox = theme.viewBox || { width: 1, height: 1 };
  const aspect = viewBox.width / viewBox.height;
  const objBottom = os.objBottom ?? 1 - os.offsetY - os.heightRatio;
  os.imgWidthRatio ??= Math.min(os.widthRatio, os.heightRatio * aspect);
  os.imgOffsetX ??= os.offsetX + Math.max(0, (os.widthRatio - os.imgWidthRatio) / 2);
  os.imgBottom ??= objBottom + Math.max(0, (os.heightRatio - os.imgWidthRatio / aspect) / 2);
  os.objBottom = objBottom;
  return os;
}

function sameBox(a, b) {
  return !!(a && b && a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height);
}

function percent(ratio) {
  return `${ratio * 100}%`;
}

function style(theme, role, file, isImage) {
  const os = objectScale(theme);
  const offset = (os.fileOffsets && os.fileOffsets[file]) || { x: 0, y: 0 };
  const scale = (os.fileScales && os.fileScales[file]) || 1;
  const fileViewBox = theme.fileViewBoxes && theme.fileViewBoxes[file];
  const mini = role.startsWith("mini-");
  const viewBox = fileViewBox || (mini && theme.miniModeViewBox) || theme.viewBox;
  const contentBox = theme.layout && theme.layout.contentBox;
  if (contentBox && (sameBox(fileViewBox, theme.viewBox) || !mini)) {
    const layout = { ...DEFAULT_LAYOUT, ...theme.layout };
    const centerX = layout.centerX ?? contentBox.x + contentBox.width / 2;
    const baselineY = layout.baselineY ?? contentBox.y + contentBox.height;
    const unit = (layout.visibleHeightRatio * scale) / contentBox.height;
    return {
      width: percent(viewBox.width * unit),
      height: isImage ? "auto" : percent(viewBox.height * unit),
      left: `calc(${percent(layout.centerXRatio - (centerX - viewBox.x) * unit)} + ${offset.x}px)`,
      bottom: `calc(${percent(layout.baselineBottomRatio - (viewBox.y + viewBox.height - baselineY) * unit)} + ${offset.y}px)`,
    };
  }
  if (isImage) {
    return {
      width: percent(os.imgWidthRatio * scale),
      height: "auto",
      left: `calc(${percent(os.imgOffsetX)} + ${offset.x}px)`,
      bottom: `calc(${percent(os.imgBottom)} + ${offset.y}px)`,
    };
  }
  return {
    width: percent(os.widthRatio),
    height: percent(os.heightRatio),
    left: `calc(${percent(os.offsetX)} + ${offset.x}px)`,
    bottom: `calc(${percent(os.objBottom)} + ${offset.y}px)`,
  };
}

function chunk(type, data) {
  const typed = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(zlib.crc32(typed));
  return Buffer.concat([length, typed, crc]);
}

function readChunks(png) {
  const chunks = [];
  for (let offset = PNG_SIGNATURE.length; offset < png.length; ) {
    const length = png.readUInt32BE(offset);
    const type = png.toString("ascii", offset + 4, offset + 8);
    chunks.push({ type, data: png.subarray(offset + 8, offset + 8 + length) });
    offset += 12 + length;
  }
  return chunks;
}

function uint32(...values) {
  const buffer = Buffer.alloc(values.length * 4);
  values.forEach((value, index) => buffer.writeUInt32BE(value, index * 4));
  return buffer;
}

function apng(frames, delays, loops) {
  const header = readChunks(frames[0]).find((c) => c.type === "IHDR");
  const width = header.data.readUInt32BE(0);
  const height = header.data.readUInt32BE(4);
  const parts = [PNG_SIGNATURE, chunk("IHDR", header.data), chunk("acTL", uint32(frames.length, loops ? 0 : 1))];
  let sequence = 0;
  frames.forEach((frame, index) => {
    const [numerator, denominator] = delays[index];
    const control = Buffer.alloc(26);
    control.writeUInt32BE(sequence++, 0);
    control.writeUInt32BE(width, 4);
    control.writeUInt32BE(height, 8);
    control.writeUInt16BE(numerator, 20);
    control.writeUInt16BE(denominator, 22);
    parts.push(chunk("fcTL", control));
    for (const { type, data } of readChunks(frame)) {
      if (type !== "IDAT") continue;
      parts.push(index === 0 ? chunk("IDAT", data) : chunk("fdAT", Buffer.concat([uint32(sequence++), data])));
    }
  });
  parts.push(chunk("IEND", Buffer.alloc(0)));
  return Buffer.concat(parts);
}

function paintAfterInvalidate(contents) {
  return new Promise((resolve) => {
    contents.once("paint", (_event, _dirty, image) => resolve(image));
    contents.invalidate();
  });
}

async function renderClip(window, theme, role, file, source) {
  const isImage = !source.endsWith(".svg");
  const contents = window.webContents;
  const call = (expression) => contents.executeJavaScript(expression);
  const runtime = theme.trustedRuntime || {};
  const scripted = (runtime.scriptedSvgFiles || []).includes(file);
  const scriptedCycleMs = scripted ? (runtime.scriptedSvgCycleMs || {})[file] : 0;
  const args = [isImage ? "apng" : "svg", pathToFileURL(source).href, style(theme, role, file, isImage), scriptedCycleMs];
  const info = await call(`loadClip(...${JSON.stringify(args)})`);
  const frames = [];
  for (let index = 0; index < info.frameCount; index += 1) {
    await call(`showFrame(${index})`);
    const image = await paintAfterInvalidate(contents);
    frames.push(image.toPNG());
  }
  return { info, png: apng(frames, await call("clipDelays()"), info.loops) };
}

async function main() {
  const [reference, output, sizeText, ...themes] = process.argv.slice(2).filter((a) => !a.startsWith("--"));
  const size = Number(sizeText);
  if (!reference || !output || !Number.isInteger(size) || themes.length === 0) throw new Error(USAGE);
  const window = new BrowserWindow({
    width: size,
    height: size,
    useContentSize: true,
    show: false,
    frame: false,
    transparent: true,
    backgroundColor: "#00000000",
    webPreferences: { offscreen: true, webSecurity: false, backgroundThrottling: false },
  });
  await window.loadFile(path.join(__dirname, "page.html"));
  for (const themeName of themes) {
    const theme = JSON.parse(fs.readFileSync(path.join(reference, "themes", themeName, "theme.json"), "utf8"));
    const directory = path.join(output, themeName);
    fs.mkdirSync(directory, { recursive: true });
    const { clips, behaviour } = plan(theme);
    fs.writeFileSync(path.join(directory, "theme.toml"), behaviourToml(behaviour));
    for (const [role, file] of clips) {
      const { info, png } = await renderClip(window, theme, role, file, sourceFile(reference, themeName, file));
      fs.writeFileSync(path.join(directory, `${role}.apng`), png);
      const kind = info.loops ? "loop" : "once";
      console.log(`${themeName}/${role} <- ${file}: ${info.frameCount} frames, ${kind}${info.lengthMs ? `, ${Math.round(info.lengthMs)} ms` : ""}`);
    }
  }
}

app.whenReady().then(main).then(
  () => app.exit(0),
  (error) => {
    console.error(error);
    app.exit(1);
  },
);
