import test from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const WOLSync = require("../lib/sync.js");

test("normalizes Nextcloud server URLs and WebDAV file paths", () => {
  assert.equal(
    WOLSync.normalizeNextcloudServerUrl("cloud.example.com/"),
    "https://cloud.example.com"
  );
  assert.equal(
    WOLSync.normalizeNextcloudServerUrl("http://localhost:8080/nextcloud/"),
    "http://localhost:8080/nextcloud"
  );
  assert.equal(
    WOLSync.nextcloudWebdavFileUrl(
      "https://cloud.example.com",
      "daniel@home",
      "OfflineWOL/wol-highlights-sync.json"
    ),
    "https://cloud.example.com/remote.php/dav/files/daniel%40home/OfflineWOL/wol-highlights-sync.json"
  );
});

test("decideSyncAction handles pull, push, merge, and noop", () => {
  assert.equal(WOLSync.decideSyncAction(true, null, null, null), "push");
  assert.equal(WOLSync.decideSyncAction(true, null, "2026-11-13T10:00:00Z", null), "push");
  assert.equal(WOLSync.decideSyncAction(true, null, null, "2026-11-13T10:00:00Z"), "merge");
  assert.equal(
    WOLSync.decideSyncAction(false, null, null, "2026-11-13T10:00:00Z"),
    "pull"
  );
  assert.equal(
    WOLSync.decideSyncAction(true, "2026-11-13T10:00:00Z", null, null),
    "push"
  );
  assert.equal(
    WOLSync.decideSyncAction(
      true,
      "2026-11-13T10:00:00Z",
      "2026-11-13T10:00:00Z",
      "2026-11-13T11:00:00Z"
    ),
    "pull"
  );
  assert.equal(
    WOLSync.decideSyncAction(
      true,
      "2026-11-13T12:00:00Z",
      "2026-11-13T10:00:00Z",
      "2026-11-13T11:00:00Z"
    ),
    "merge"
  );
  assert.equal(
    WOLSync.decideSyncAction(
      true,
      "2026-11-13T10:00:00Z",
      "2026-11-13T10:00:00Z",
      "2026-11-13T10:00:00Z"
    ),
    "noop"
  );
});

test("mergeMarks combines highlights, notes, and inputFields", () => {
  const local = {
    version: 1,
    docs: { 100: { 2: [[0, 3, 1]] } },
    bible: {},
    notes: {
      docs: {
        100: {
          2: [{ guid: "n1", title: "Local", content: "Hello", lastModified: "2026-11-13T10:00:00Z" }],
        },
      },
      bible: {},
    },
    inputFields: {
      docs: { 100: { q1: "Answer A" } },
      bible: {},
    },
  };
  const remote = {
    version: 1,
    docs: {
      100: { 2: [[0, 3, 2], [5, 8, 3]] },
      200: { 4: [[1, 2, 1]] },
    },
    bible: {},
    notes: {
      docs: {
        100: {
          2: [{ guid: "n1", title: "Remote", content: "Updated", lastModified: "2026-11-13T12:00:00Z" }],
        },
      },
      bible: {},
    },
    inputFields: {
      docs: { 200: { q2: "Answer B" } },
      bible: {},
    },
  };

  const merged = WOLSync.mergeMarks(local, remote);
  assert.equal(merged.highlightCount, 3);
  assert.equal(merged.noteCount, 3);
  assert.equal(merged.documentCount, 2);
  assert.equal(merged.notes.docs["100"]["2"][0].title, "Remote");
  assert.equal(merged.inputFields.docs["100"].q1, "Answer A");
  assert.equal(merged.inputFields.docs["200"].q2, "Answer B");
});
