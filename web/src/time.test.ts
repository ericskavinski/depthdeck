import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { formatDuration } from "./time.ts";

describe("formatDuration", () => {
  it("formats monotonic time without consulting wall clock state", () => {
    assert.equal(formatDuration(61_234), "01:01.234");
  });
});
