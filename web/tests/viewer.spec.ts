import { expect, test } from "@playwright/test";

test("reads a selected tape through the real worker", async ({ page }) => {
  const tapeRequests: string[] = [];
  page.on("request", (request) => {
    if (new URL(request.url()).pathname.endsWith(".ddt")) {
      tapeRequests.push(request.url());
    }
  });

  await page.goto("./");
  await expect(page.getByText("NO TAPE", { exact: true })).toBeVisible();
  expect(tapeRequests).toEqual([]);

  await page.locator('input[type="file"]').setInputFiles({
    name: "invalid.ddt",
    mimeType: "application/octet-stream",
    buffer: Buffer.alloc(0),
  });

  await expect(page.getByText("invalid.ddt", { exact: true })).toBeVisible();
  await expect(page.locator(".error")).toBeVisible();
  expect(tapeRequests).toEqual([]);
});
