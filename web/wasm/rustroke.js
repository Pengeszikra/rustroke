/**
 * WASM Loader for Rustroke
 * This module loads and initializes the WebAssembly module
 */

let wasmInstance = null;
let wasmMemory = null;

/**
 * Initialize and load the WASM module
 * @returns {Promise<Object>} The WASM instance exports
 */
export default async function init() {
  if (wasmInstance) {
    return wasmInstance.exports;
  }

  const wasmUrl = new URL('./rustroke.wasm', import.meta.url);
  
  const importObject = {
    env: {
      // Add any required imports here if needed
    }
  };

  try {
    let result;
    
    if (typeof WebAssembly.instantiateStreaming === 'function') {
      // Use streaming compilation if available (faster)
      result = await WebAssembly.instantiateStreaming(
        fetch(wasmUrl),
        importObject
      );
    } else {
      // Fallback for Safari and older browsers
      const response = await fetch(wasmUrl);
      const buffer = await response.arrayBuffer();
      result = await WebAssembly.instantiate(buffer, importObject);
    }

    wasmInstance = result.instance;
    wasmMemory = wasmInstance.exports.memory;
    
    return wasmInstance.exports;
  } catch (err) {
    console.error('Failed to load WASM module:', err);
    throw err;
  }
}

/**
 * Get the WASM memory buffer
 * @returns {WebAssembly.Memory}
 */
export function getMemory() {
  return wasmMemory;
}

/**
 * Get the WASM instance
 * @returns {WebAssembly.Instance}
 */
export function getInstance() {
  return wasmInstance;
}
