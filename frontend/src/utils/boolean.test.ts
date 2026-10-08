import { test } from "node:test";
import assert from "node:assert/strict";
import { parseBoolean } from "./boolean.ts";

test("environment strings become booleans", () => {
  for (const value of ["true", "TRUE", " yes ", "1"]) {
    assert.equal(parseBoolean(value, false), true, value);
  }
  for (const value of ["false", "False", "no", "0"]) {
    assert.equal(parseBoolean(value, true), false, value);
  }
});

test("real booleans pass through", () => {
  assert.equal(parseBoolean(true, false), true);
  assert.equal(parseBoolean(false, true), false);
});

test("missing or unreadable values use the default", () => {
  for (const value of [undefined, null, "", "maybe", 2]) {
    assert.equal(parseBoolean(value, true), true, String(value));
    assert.equal(parseBoolean(value, false), false, String(value));
  }
});
