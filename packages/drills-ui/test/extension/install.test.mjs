import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { lib } from "./lib.mjs";

const { INSTALL, locate, projectCopy, projectDir, projectRepo, sessionRepo } = await lib("paths.mjs");
const { pluginVersion, shippedVersion } = await lib("build-info.mjs");

function scratch(t) {
    const dir = realpathSync(mkdtempSync(join(tmpdir(), "drills-install-")));
    t.after(() => rmSync(dir, { recursive: true, force: true }));
    return dir;
}

test("a committed copy serves the repository it is committed in", () => {
    const repo = join("work", "app");
    assert.equal(projectRepo(join(repo, ".github", "extensions", "auditaur-drills")), repo);
    assert.equal(INSTALL, "project");
});

test("plugin and user installs are not committed copies", () => {
    const plugin = join("cache", "auditaur", "com.github.copilot", "extensions", "auditaur-drills");
    assert.equal(projectRepo(plugin), null);
    assert.equal(projectRepo(join("home", ".copilot", "extensions", "auditaur-drills")), null);
});

test("an installed copy serves the git root of the session's working directory", (t) => {
    const repo = scratch(t);
    execFileSync("git", ["init", "-q"], { cwd: repo });
    const sub = join(repo, "src", "deep");
    mkdirSync(sub, { recursive: true });
    assert.equal(sessionRepo(sub).toLowerCase(), repo.toLowerCase());
});

test("outside a repository the working directory itself is served", (t) => {
    const dir = scratch(t);
    assert.equal(sessionRepo(dir), dir);
});

test("the committed copy an installed copy defers to", () => {
    assert.equal(projectCopy("repo"), join("repo", ".github", "extensions", "auditaur-drills", "extension.mjs"));
});

function committed(repo) {
    mkdirSync(projectDir(repo), { recursive: true });
    writeFileSync(projectCopy(repo), "");
}

test("an installed copy steps aside only when the repository commits its own", (t) => {
    const repo = scratch(t);
    const plugin = join(scratch(t), "extensions", "auditaur-drills");
    assert.deepEqual(locate(plugin, () => repo), { install: "installed", repo, defers: false });
    committed(repo);
    assert.deepEqual(locate(plugin, () => repo), { install: "installed", repo, defers: true });
});

test("a committed copy reached through a link or another casing does not defer to itself", (t) => {
    const repo = scratch(t);
    committed(repo);
    const link = join(scratch(t), "linked");
    symlinkSync(projectDir(repo), link, "junction");
    assert.deepEqual(locate(link, () => repo), { install: "project", repo, defers: false });
    if (process.platform === "win32") {
        assert.equal(locate(projectDir(repo).toUpperCase(), () => repo).install, "project");
    }
});

test("only the auditaur plugin manifest names the version", async (t) => {
    const plugin = scratch(t);
    const extension = join(plugin, "com.github.copilot", "extensions", "auditaur-drills");
    mkdirSync(extension, { recursive: true });
    assert.equal(await pluginVersion(extension), null);

    writeFileSync(join(plugin, "plugin.json"), JSON.stringify({ name: "other", version: "9.9.9" }));
    assert.equal(await pluginVersion(extension), null);

    writeFileSync(join(plugin, "plugin.json"), JSON.stringify({ name: "auditaur", version: "1.2.3" }));
    assert.equal(await pluginVersion(extension), "1.2.3");
});

test("an `init extension` copy names its version from the install stamp; the plugin wins", async (t) => {
    const plugin = scratch(t);
    const extension = join(plugin, "com.github.copilot", "extensions", "auditaur-drills");
    mkdirSync(extension, { recursive: true });
    assert.deepEqual(await shippedVersion(extension), { version: null, source: null });

    writeFileSync(join(extension, "installed.json"), "not json");
    assert.deepEqual(await shippedVersion(extension), { version: null, source: null });

    writeFileSync(join(extension, "installed.json"), JSON.stringify({ version: "0.4.15" }));
    assert.deepEqual(await shippedVersion(extension), { version: "0.4.15", source: "init" });

    writeFileSync(join(plugin, "plugin.json"), JSON.stringify({ name: "auditaur", version: "0.4.16" }));
    assert.deepEqual(await shippedVersion(extension), { version: "0.4.16", source: "plugin" });
});
