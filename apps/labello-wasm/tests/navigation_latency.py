"""Opt-in click-to-ready timing for the fixed 1440x1000 workflow fixture.

The probe observes the new working-image draw, a successful display validation,
and an enabled primary button, then waits for the next animation frame. It keeps
pixels and requests in the browser and exports only durations. The viewport and
DPR are deliberately fixed because the readiness pixel belongs to the fixture.
"""
import asyncio
import time

PROBE = r"""(() => {
  const probe = window.__navigationProbe = {serial: 0, armed: false};
  document.addEventListener('pointerup', event => {
    if (!probe.armed || !event.isTrusted) return;
    probe.click = event.timeStamp;
    probe.before = probe.serial;
    probe.display = false;
    probe.image = false;
    probe.ready = null;
  }, true);
  const fetch = window.fetch;
  window.fetch = function(input, ...args) {
    const path = new URL(typeof input === 'string' || input instanceof URL ? input : input.url, location.href).pathname;
    const click = probe.click;
    return fetch.call(this, input, ...args).then(response => {
      if (path.endsWith('/work-items/display') && response.ok && probe.armed && probe.click === click)
        probe.display = true;
      return response;
    });
  };
  for (const Class of [window.WebGLRenderingContext, window.WebGL2RenderingContext]) {
    if (!Class) continue;
    const textures = new WeakMap();
    const upload = Class.prototype.texImage2D;
    Class.prototype.texImage2D = function(...args) {
      const result = upload.apply(this, args);
      if (args[3] > 100 && args[4] > 100 && args[3] <= 1280 && args[4] <= 1280 && args[8])
        textures.set(this.getParameter(this.TEXTURE_BINDING_2D), ++probe.serial);
      return result;
    };
    const draw = Class.prototype.drawElements;
    Class.prototype.drawElements = function(...args) {
      const result = draw.apply(this, args);
      if (!probe.armed || probe.ready != null || !probe.click) return result;
      if ((textures.get(this.getParameter(this.TEXTURE_BINDING_2D)) || 0) > probe.before)
        probe.image = true;
      if (probe.image && probe.display && !probe.pending) {
        probe.pending = true;
        const click = probe.click;
        queueMicrotask(() => {
          probe.pending = false;
          if (!probe.armed || click !== probe.click) return;
          const pixel = new Uint8Array(4);
          this.readPixels(this.drawingBufferWidth - 25, 40, 1, 1, this.RGBA, this.UNSIGNED_BYTE, pixel);
          if (pixel[0] < 80 && pixel[1] > 130 && pixel[2] > 110) {
            requestAnimationFrame(() => {
              if (probe.armed && probe.click === click && probe.ready == null)
                probe.ready = performance.now() - click;
            });
          }
        });
      }
      return result;
    };
  }
})();
"""


class NavigationLatency:
    def __init__(self, page, budget_ms):
        self.page = page
        self.budget_ms = budget_ms
        self.samples = []

    async def click(self, phase, action):
        await self.page.evaluate("Object.assign(window.__navigationProbe, {armed: true, click: null, ready: null, pending: false})")
        await self.page.mouse.click(60 if action == "previous" else 1380, 965)
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            elapsed = await self.page.evaluate("window.__navigationProbe.ready")
            if elapsed is not None:
                await self.page.evaluate("window.__navigationProbe.armed = false")
                self.samples.append({"phase": phase, "action": action, "clickToReadyMs": round(elapsed, 1)})
                return
            await asyncio.sleep(0.01)
        raise AssertionError("next-item-not-rendered-ready")

    def within_budget(self):
        return self.samples and all(sample["clickToReadyMs"] < self.budget_ms for sample in self.samples)
