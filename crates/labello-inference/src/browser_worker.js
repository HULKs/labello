// Browser adapter only: preprocessing, decoding and hint policy live in Rust.
// Each run owns a worker so cancellation stops CPU execution and frees model memory.
function workerMain() {
  self.onmessage = async ({ data: { model, input, size, gpu, runtime, outputName } }) => {
    let session;
    try {
      importScripts(new URL("ort.webgpu.min.js", runtime).href);
      ort.env.wasm.wasmPaths = runtime;
      ort.env.wasm.numThreads = 1;
      ort.env.logLevel = "fatal";
      if (gpu && !self.navigator.gpu) throw new Error("WebGPU unavailable");
      session = await ort.InferenceSession.create(model, {
        executionProviders: gpu ? ["webgpu"] : ["wasm"], logSeverityLevel: 4,
      });
      const metadata = session.inputMetadata[0];
      const expected = [1, 3, size, size];
      if (session.inputNames.length !== 1 ||
          metadata.type !== "float32" || metadata.shape.length !== 4 ||
          !metadata.shape.every((dimension, index) => dimension === expected[index])) {
        throw new Error("Unsupported model input");
      }
      const name = outputName || (session.outputNames.length === 1 ? session.outputNames[0] : null);
      if (!name || !session.outputNames.includes(name)) throw new Error("Selected model output unavailable");
      const outputs = await session.run({ [session.inputNames[0]]: new ort.Tensor("float32", input, expected) }, [name]);
      const tensor = outputs[name];
      if (tensor.type !== "float32" || tensor.size > 8000000) throw new Error("Unsupported model output");
      const data = new Float32Array(await tensor.getData());
      self.postMessage({ shape: tensor.dims, data }, [data.buffer]);
      tensor.dispose();
    } catch {
      // Runtime errors may contain model internals. Return a bounded category only.
      self.postMessage({ failed: true });
    } finally {
      if (session) await session.release();
    }
  };
}

export function start(model, input, size, gpu, runtime, outputName) {
  const url = URL.createObjectURL(new Blob([`(${workerMain.toString()})();`], { type: "text/javascript" }));
  const worker = new Worker(url);
  URL.revokeObjectURL(url);
  let timeout, rejectRun;
  let done = false;
  const stop = () => { clearTimeout(timeout); worker.terminate(); };
  const promise = new Promise((resolve, reject) => {
    rejectRun = reject;
    timeout = setTimeout(() => { done = true; stop(); reject(new Error("Inference timeout")); }, 120000);
    worker.onerror = () => { done = true; stop(); reject(new Error("Inference worker failed")); };
    worker.onmessage = ({ data }) => {
      if (done) return;
      done = true; stop();
      if (data.failed) reject(new Error("Model execution failed")); else resolve(data);
    };
    worker.postMessage({ model, input, size, gpu, runtime, outputName }, [model.buffer, input.buffer]);
  });
  return { promise, cancel() { stop(); if (!done) { done = true; rejectRun(new Error("Inference cancelled")); } } };
}
