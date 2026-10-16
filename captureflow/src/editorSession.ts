export type ImageDocument<T> = {
  imagePath: string;
  width: number;
  height: number;
  offsetX: number;
  offsetY: number;
  objects: T[];
};

/** Text fields own their editing shortcuts, including fields outside the canvas. */
export function ownsEditingKeys(target: EventTarget | null): boolean {
  return target instanceof Element && !!target.closest(
    'input, textarea, select, [contenteditable]:not([contenteditable="false"]), [role="textbox"], [role="spinbutton"], [role="dialog"]',
  );
}

export type SaveState = "pending" | "saving" | "saved" | "error";

/** Coalesce edits and serialize writes so an older save cannot replace a newer one. */
export class AutoSaveQueue<T> {
  private latest: { revision: number; value: T } | undefined;
  private revision = 0;
  private savedRevision = 0;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private running: Promise<void> | undefined;

  constructor(
    private readonly write: (value: T) => Promise<unknown>,
    private readonly status: (state: SaveState, error?: unknown) => void,
    private readonly delay = 600,
  ) {}

  schedule(value: T) {
    this.latest = { revision: ++this.revision, value };
    clearTimeout(this.timer);
    this.status("pending");
    this.timer = setTimeout(() => { void this.flush().catch(() => {}); }, this.delay);
  }

  async flush(): Promise<void> {
    clearTimeout(this.timer);
    if (this.running) {
      await this.running;
      return this.flush();
    }
    if (!this.latest || this.latest.revision === this.savedRevision) return;
    this.running = this.drain();
    try { await this.running; }
    finally { this.running = undefined; }
  }

  private async drain() {
    try {
      while (this.latest && this.latest.revision !== this.savedRevision) {
        const next = this.latest;
        this.status("saving");
        await this.write(next.value);
        this.savedRevision = next.revision;
      }
      this.status("saved");
    } catch (error) {
      this.status("error", error);
      throw error;
    }
  }

  cancelTimer() { clearTimeout(this.timer); }
}
