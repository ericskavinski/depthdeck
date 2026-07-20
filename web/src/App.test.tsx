import assert from "node:assert/strict";
import { describe, it } from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import App from "./App";

describe("initial viewer state", () => {
  it("requires the user to select a local tape", () => {
    const markup = renderToStaticMarkup(<App />);

    assert.match(markup, /NO TAPE/);
    assert.match(markup, /no tape loaded/);
    assert.match(markup, /type="file" accept="\.ddt"/);
    assert.match(markup, /disabled="" aria-label="Play replay"/);
  });
});
