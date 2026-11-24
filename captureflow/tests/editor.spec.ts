import { test, expect, type Page } from "@playwright/test";

// Real React/Canvas interaction with deterministic native I/O. Portable QA is separate.
test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    const w = window as any;
    let id = 0;
    const callbacks = new Map();
    const listeners = new Map();
    const images = new Map([["capture.png", { width: 600, height: 400 }]]);
    const resources = new Map();
    w.qa = { calls: [], saved: [], history: [], failSave: false, failRepeat: false, confirm: false, saveDelay: 0, activeWrites: 0, maxWrites: 0 };
    localStorage.setItem("captureflow-update-check", String(Date.now()));
    w.qa.emit = (event: string, payload?: unknown) => {
      for (const [key, value] of listeners) if (value.event === event) callbacks.get(value.handler)?.({ event, id: key, payload });
    };
    w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: (_: string, key: number) => listeners.delete(key) };
    w.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" } },
      transformCallback: (fn: any) => { callbacks.set(++id, fn); return id; },
      invoke: async (command: string, args: any = {}) => {
        w.qa.calls.push({ command, args });
        switch (command) {
          case "plugin:app|version": return "0.5.14";
          case "get_settings": return { captureShortcut: "Alt+Shift+A", historyLimit: 20, language: "zh-TW", defaultSaveDirectory: "", launchAtStartup: false };
          case "list_monitors": return [{ deviceName: "DISPLAY1" }];
          case "list_capture_history": return w.qa.history;
          case "load_annotation_project_from": {
            const saved = w.qa.saved.at(-1);
            return { sourceImage: saved.imagePath, canvasWidth: saved.canvasWidth, canvasHeight: saved.canvasHeight, objects: saved.objects };
          }
          case "plugin:event|listen": listeners.set(++id, args); return id;
          case "plugin:event|unlisten": return;
          case "capture_monitor":
          case "select_screen_area": return { imagePath: "capture.png", metadataPath: "capture.json", width: 600, height: 400, selection: { x: -600, y: 0, width: 600, height: 400 } };
          case "repeat_history_selection": {
            if (w.qa.failRepeat) throw new Error("擷取範圍已超出目前螢幕，請重新框選。");
            const { width, height } = args.selection;
            images.set("repeat.png", { width, height });
            return { imagePath: "repeat.png", metadataPath: "repeat.json", width, height, selection: args.selection };
          }
          case "plugin:image|from_path": resources.set(++id, images.get(args.path)); return id;
          case "plugin:image|rgba": {
            const size = resources.get(args.rid);
            return new Array(size.width * size.height * 4).fill(255);
          }
          case "plugin:resources|close": return;
          case "auto_save_annotation_project":
            w.qa.activeWrites++;
            w.qa.maxWrites = Math.max(w.qa.maxWrites, w.qa.activeWrites);
            await new Promise(resolve => setTimeout(resolve, w.qa.saveDelay));
            w.qa.activeWrites--;
            if (w.qa.failSave) throw new Error("simulated disk failure");
            w.qa.saved.push(structuredClone(args));
            return "history/capture.captureflow.json";
          case "save_composited_image": {
            const path = `crop-${++id}.png`;
            images.set(path, { width: args.width, height: args.height });
            return path;
          }
          case "plugin:dialog|message": return args.buttons.OkCancelCustom[w.qa.confirm ? 0 : 1];
          case "clear_capture_history": return;
          case "plugin:window|minimize":
          case "plugin:window|unminimize":
          case "plugin:window|set_focus":
          case "plugin:window|hide":
          case "plugin:window|destroy": return;
          default: throw new Error(`Unmocked native command: ${command}`);
        }
      },
    };
  });
  await page.goto("/");
  await page.getByRole("button", { name: "螢幕 1", exact: true }).click();
  await expect(page.locator(".annotation-editor")).toHaveAttribute("aria-busy", "false");
});

async function draw(page: Page, x = 0.2, y = 0.2) {
  await page.getByRole("button", { name: "矩形", exact: true }).click();
  const box = (await page.locator(".annotation-stage svg").boundingBox())!;
  await page.mouse.move(box.x + box.width * x, box.y + box.height * y);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width * (x + 0.15), box.y + box.height * (y + 0.15));
  await page.mouse.up();
}

test("crop undo/redo restores source, latest annotations and preceding edit history", async ({ page }) => {
  await draw(page);
  await page.getByRole("button", { name: "裁切", exact: true }).click();
  const box = (await page.locator(".annotation-stage svg").boundingBox())!;
  await page.mouse.move(box.x + box.width * 0.1, box.y + box.height * 0.1);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width * 0.8, box.y + box.height * 0.8);
  await page.mouse.up();
  await page.getByRole("button", { name: "套用裁切", exact: true }).click();
  await expect(page.locator(".annotation-editor")).toHaveAttribute("aria-busy", "false");
  await expect(page.locator("[data-object-id]")).toHaveCount(1);
  const cropped = await page.locator(".current-image-card strong").innerText();
  expect(cropped).not.toBe("600 × 400");
  await page.getByRole("button", { name: "復原", exact: true }).click();
  await expect(page.locator(".current-image-card strong")).toHaveText("600 × 400");
  await expect(page.locator("[data-object-id]")).toHaveCount(1);
  await page.getByRole("button", { name: "復原", exact: true }).click();
  await expect(page.locator("[data-object-id]")).toHaveCount(0);
  await page.getByRole("button", { name: "重做", exact: true }).click();
  await expect(page.locator("[data-object-id]")).toHaveCount(1);
  await page.getByRole("button", { name: "重做", exact: true }).click();
  await expect(page.locator(".current-image-card strong")).toHaveText(cropped);
  await expect(page.locator("[data-object-id]")).toHaveCount(1);
  await page.getByRole("button", { name: "關閉圖片", exact: true }).click();
  expect(await page.evaluate(() => (window as any).qa.saved.at(-1).historyImagePath)).toBe("capture.png");
  await page.evaluate(() => {
    const qa = (window as any).qa, saved = qa.saved.at(-1);
    qa.history = [{ projectPath: "history/capture.captureflow.json", imagePath: saved.imagePath,
      createdAtUnixMs: Date.now(), canvasWidth: saved.canvasWidth, canvasHeight: saved.canvasHeight }];
  });
  await page.locator(".history-toggle").click();
  await page.locator(".history-entry").click();
  await expect(page.locator("[data-object-id]")).toHaveCount(1);
  await page.getByRole("button", { name: "關閉圖片", exact: true }).click();
  expect(await page.evaluate(() => (window as any).qa.saved.at(-1).historyProjectPath)).toBe("history/capture.captureflow.json");
});

test("closing immediately flushes the latest edit before unmount", async ({ page }) => {
  await draw(page);
  await page.getByRole("button", { name: "關閉圖片", exact: true }).click();
  await expect(page.locator(".annotation-editor")).toHaveCount(0);
  expect(await page.evaluate(() => (window as any).qa.saved.at(-1).objects.length)).toBe(1);
});

test("save failure prevents closing and offers retry", async ({ page }) => {
  await page.evaluate(() => { (window as any).qa.failSave = true; });
  await draw(page);
  await page.getByRole("button", { name: "關閉圖片", exact: true }).click();
  await expect(page.getByRole("button", { name: "重試保存" })).toBeVisible();
  await expect(page.locator("[data-object-id]")).toHaveCount(1);
  await page.evaluate(() => { (window as any).qa.failSave = false; });
  await page.getByRole("button", { name: "重試保存" }).click();
  await expect(page.locator(".autosave-status")).toContainText("已儲存");
  await page.getByRole("button", { name: "關閉圖片", exact: true }).click();
  await expect(page.locator(".annotation-editor")).toHaveCount(0);
});

test("Backspace, Delete and Ctrl+Z in settings do not edit selected objects", async ({ page }) => {
  await draw(page);
  await page.getByText("偏好設定", { exact: true }).click();
  const input = page.locator("#capture-shortcut");
  await input.fill("Alt+Shift+AB");
  await input.press("Backspace");
  await expect(input).toHaveValue("Alt+Shift+A");
  await input.press("Delete");
  await input.press("Control+z");
  await expect(page.locator("[data-object-id]")).toHaveCount(1);
  await page.getByRole("button", { name: "復原", exact: true }).click();
  await expect(page.locator("[data-object-id]")).toHaveCount(0);
});

test("clear history cancellation keeps editor; confirmation closes after save", async ({ page }) => {
  await draw(page);
  await page.getByText("偏好設定", { exact: true }).click();
  await page.getByRole("button", { name: "清除歷史擷圖記錄", exact: true }).click();
  await expect(page.locator("[data-object-id]")).toHaveCount(1);
  expect(await page.evaluate(() => (window as any).qa.calls.some((c: any) => c.command === "clear_capture_history"))).toBe(false);
  await page.evaluate(() => { (window as any).qa.confirm = true; });
  await page.getByRole("button", { name: "清除歷史擷圖記錄", exact: true }).click();
  await expect(page.locator(".annotation-editor")).toHaveCount(0);
  expect(await page.evaluate(() => (window as any).qa.saved.at(-1).objects.length)).toBe(1);
});

test("quit awaits the latest save", async ({ page }) => {
  await draw(page);
  await page.evaluate(() => { (window as any).qa.emit("tauri://close-requested"); });
  await page.getByRole("button", { name: "結束應用程式", exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).qa.calls.at(-1).command)).toBe("plugin:window|destroy");
  expect(await page.evaluate(() => (window as any).qa.saved.at(-1).objects.length)).toBe(1);
});

test("light footer links meet project contrast in normal and hover states", async ({ page }) => {
  const link = page.getByRole("link", { name: "述文老師部落格", exact: true });
  for (const hover of [false, true]) {
    if (hover) await link.hover();
    const ratio = await link.evaluate((element) => {
      const luminance = (color: string) => {
        const channels = color.match(/[\d.]+/g)!.slice(0, 3).map(Number).map(c => c / 255).map(c => c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
        return channels[0] * .2126 + channels[1] * .7152 + channels[2] * .0722;
      };
      const fg = luminance(getComputedStyle(element).color);
      const bg = luminance(getComputedStyle(document.querySelector(".app-shell")!).backgroundColor);
      return (Math.max(fg, bg) + .05) / (Math.min(fg, bg) + .05);
    });
    expect(ratio).toBeGreaterThanOrEqual(4.5);
  }
});

test("slow autosave serializes writes and flush includes edits made during a write", async ({ page }) => {
  const result = await page.evaluate(async () => {
    // @ts-expect-error Vite serves this module in the browser.
    const { AutoSaveQueue } = await import("/src/editorSession.ts");
    const written: number[] = [];
    let active = 0, maximum = 0;
    const queue = new AutoSaveQueue(async (value: number) => {
      maximum = Math.max(maximum, ++active);
      await new Promise(resolve => setTimeout(resolve, 40));
      written.push(value);
      active--;
    }, () => {}, 600);
    queue.schedule(1);
    const first = queue.flush();
    queue.schedule(2);
    queue.schedule(3);
    const second = queue.flush();
    await Promise.all([first, second]);
    queue.cancelTimer();
    return { written, maximum };
  });
  expect(result).toEqual({ written: [1, 3], maximum: 1 });
});

test("global capture replacement flushes the previous document", async ({ page }) => {
  await draw(page);
  await page.evaluate(() => (window as any).qa.emit("captureflow://selection-complete", {
    imagePath: "capture.png", metadataPath: "capture.json", width: 600, height: 400,
    selection: { x: 0, y: 0, width: 600, height: 400 },
  }));
  await expect(page.locator("[data-object-id]")).toHaveCount(0);
  expect(await page.evaluate(() => (window as any).qa.saved.some((s: any) => s.objects.length === 1))).toBe(true);
});

const savedRange = { x: -300, y: 40, width: 250, height: 150 };

async function seedRepeatHistory(page: Page) {
  await page.evaluate((selection) => {
    const entry = { imagePath: "capture.png", canvasWidth: 600, canvasHeight: 400, createdAtUnixMs: Date.now() };
    const canvas = document.createElement("canvas");
    canvas.width = 50;
    canvas.height = 30;
    const ctx = canvas.getContext("2d")!;
    ctx.fillStyle = "#087346";
    ctx.fillRect(0, 0, 50, 30);
    (window as any).qa.history = [
      { ...entry, projectPath: "range.json", selection, rangeThumbnailDataUrl: canvas.toDataURL() },
      { ...entry, projectPath: "imported.json", selection: null },
    ];
  }, savedRange);
}

test("history picker cancels, forwards desktop coordinates, and preserves the editor on failure", async ({ page }) => {
  await draw(page);
  const trigger = page.getByRole("button", { name: "重複歷史擷取範圍" });
  await trigger.click();
  const dialog = page.getByRole("dialog", { name: "重複歷史擷取範圍" });
  await expect(dialog.getByRole("status")).toHaveText("尚無可重複擷取的範圍，請先完成一次螢幕擷取。");
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(trigger).toBeFocused();
  await seedRepeatHistory(page);
  await page.evaluate(() => { (window as any).qa.failRepeat = true; });
  await trigger.click();
  const tile = dialog.getByRole("button", { name: /250 × 150 px.*X -300 · Y 40/ });
  await expect(dialog.locator(".repeat-area-card")).toHaveCount(1);
  await expect(tile.locator("img")).toBeVisible();
  await dialog.screenshot({ path: "../qa-artifacts/history-repeat-preview.png" });
  await tile.click();
  await expect(page.getByRole("alert")).toContainText("擷取範圍已超出目前螢幕");
  await expect(page.locator("[data-object-id]")).toHaveCount(1);
  await page.evaluate(() => { (window as any).qa.failRepeat = false; });
  await trigger.click();
  await tile.click();
  await expect(page.locator(".annotation-stage canvas").first()).toHaveAttribute("width", "250");
  await expect(page.locator("[data-object-id]")).toHaveCount(0);
  const calls = await page.evaluate(() => (window as any).qa.calls as { command: string; args: unknown }[]);
  const repeat = calls.findIndex(({ command }) => command === "repeat_history_selection");
  expect(calls[repeat].args).toEqual({ selection: savedRange });
  const windowCommands = calls.map(({ command }) => command).filter((command) =>
    ["plugin:window|minimize", "repeat_history_selection", "plugin:window|unminimize"].includes(command),
  );
  const captureSequence = ["plugin:window|minimize", "repeat_history_selection", "plugin:window|unminimize"];
  expect(windowCommands).toEqual([...captureSequence, ...captureSequence]);
  expect(calls.slice(0, repeat).some(({ command }) => command === "auto_save_annotation_project")).toBe(true);
  expect(calls.some(({ command }) => command === "load_annotation_project_from")).toBe(false);
});

test("visible interface text meets 4.5:1 in light/dark normal, hover, selected, disabled and error states", async ({ page }) => {
  async function check() {
    const failures = await page.evaluate(() => {
      const rgba = (text: string) => {
        const c = text.match(/[\d.]+/g)!.map(Number);
        return [c[0], c[1], c[2], c[3] ?? 1];
      };
      const blend = (fg: number[], bg: number[]) => [0, 1, 2].map(i => fg[i] * fg[3] + bg[i] * (1 - fg[3])).concat(1);
      const luminance = (c: number[]) => {
        const s = c.slice(0, 3).map(c => c / 255).map(c => c <= .04045 ? c / 12.92 : ((c + .055) / 1.055) ** 2.4);
        return .2126 * s[0] + .7152 * s[1] + .0722 * s[2];
      };
      const failures: unknown[] = [];
      for (const el of document.querySelectorAll<HTMLElement>("body *")) {
        if (el.closest("svg, .annotation-stage, .zoom-minimap") || !el.checkVisibility({ checkOpacity: true, checkVisibilityCSS: true })) continue;
        const text = [...el.childNodes].filter(n => n.nodeType === Node.TEXT_NODE).map(n => n.textContent).join("").trim();
        if (!text || el.getBoundingClientRect().width < 2 || el.getBoundingClientRect().height < 2) continue;
        let bg = [255, 255, 255, 1];
        const ancestors: Element[] = [];
        for (let p: Element | null = el; p; p = p.parentElement) ancestors.unshift(p);
        for (const p of ancestors) bg = blend(rgba(getComputedStyle(p).backgroundColor), bg);
        const fg = blend(rgba(getComputedStyle(el).color), bg);
        const a = luminance(fg), b = luminance(bg);
        const ratio = (Math.max(a, b) + .05) / (Math.min(a, b) + .05);
        if (ratio < 4.5) failures.push({ text: text.slice(0, 50), ratio, color: getComputedStyle(el).color, bg });
      }
      return failures;
    });
    expect(failures).toEqual([]);
  }
  await page.getByText("偏好設定", { exact: true }).click();
  for (const theme of ["light", "dark"]) {
    await page.getByLabel("色系主題").selectOption(theme);
    await check();
    await page.getByRole("button", { name: "開啟 JSON 專案", exact: true }).hover();
    await check();
    await page.getByRole("button", { name: "文字", exact: true }).click();
    await check();
    const repeat = page.getByRole("button", { name: "重複歷史擷取範圍" });
    await repeat.hover();
    await check();
    await repeat.click();
    const picker = page.getByRole("dialog", { name: "重複歷史擷取範圍" });
    await expect(picker.getByText("尚無可重複擷取的範圍", { exact: false })).toBeVisible();
    await check();
    await page.keyboard.press("Escape");
    await seedRepeatHistory(page);
    await repeat.click();
    const card = picker.locator(".repeat-area-card");
    await card.hover();
    await check();
    await page.mouse.down();
    await check();
    await page.mouse.move(0, 0);
    await page.mouse.up();
    await page.keyboard.press("Escape");
    await page.evaluate(() => { (window as any).qa.history = []; });
  }
  await page.evaluate(() => { (window as any).qa.failSave = true; });
  await draw(page);
  await page.getByRole("button", { name: "關閉圖片", exact: true }).click();
  await expect(page.getByRole("button", { name: "重試保存" })).toBeVisible();
  for (const theme of ["light", "dark"]) {
    await page.getByLabel("色系主題").selectOption(theme);
    await check();
  }
  await page.screenshot({ path: "../qa-artifacts/editor-dark-save-error.png", fullPage: true });
});
