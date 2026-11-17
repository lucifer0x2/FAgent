import assert from "node:assert/strict";
import test from "node:test";
import {
  PANEL_MAX_WIDTH,
  PANEL_MIN_WIDTH,
  panelKeyWidth,
  visiblePanelMax,
} from "../src/hooks/usePanelWidth.ts";

test("the visible panel cap follows workspace.css max(200px, 50vw - 160px) within the limits", () => {
  assert.equal(visiblePanelMax(1120), 400);
  assert.equal(visiblePanelMax(1121), 400, "whole pixels");
  assert.equal(visiblePanelMax(801), 240);
  assert.equal(visiblePanelMax(600), PANEL_MIN_WIDTH, "never below the minimum");
  assert.equal(visiblePanelMax(2400), PANEL_MAX_WIDTH, "wide screens keep the stored maximum");
});

test("resize keys step from the rendered width and stay within the visible cap", () => {
  const max = visiblePanelMax(1120); // 400 px, while 500 px may be stored
  // A stored 500 px renders at 400 px: the first step narrows what is on screen.
  assert.equal(panelKeyWidth("ArrowLeft", 400, 1, max), 384);
  assert.equal(panelKeyWidth("ArrowRight", 400, 1, max), 400, "no growth past the cap");
  assert.equal(panelKeyWidth("ArrowRight", 384, 1, max), 400);
  // The right panel's edge widens it leftwards.
  assert.equal(panelKeyWidth("ArrowLeft", 300, -1, max), 316);
  assert.equal(panelKeyWidth("ArrowRight", 300, -1, max), 284);
  assert.equal(panelKeyWidth("ArrowLeft", PANEL_MIN_WIDTH, 1, max), PANEL_MIN_WIDTH);
  assert.equal(panelKeyWidth("Home", 300, 1, max), PANEL_MIN_WIDTH);
  assert.equal(panelKeyWidth("End", 300, 1, max), max, "End is the widest width on screen");
  assert.equal(panelKeyWidth("End", 300, 1, visiblePanelMax(2400)), PANEL_MAX_WIDTH);
  assert.equal(panelKeyWidth("Enter", 300, 1, max), undefined);
});
