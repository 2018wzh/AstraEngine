const MAX_DTO_BYTES = 4 * 1024 * 1024;
const MAX_MEMORY_BYTES = 64 * 1024 * 1024;
const HASH_BYTES = 32;

async function sha256(bytes) {
    const hash = await crypto.subtle.digest("SHA-256", bytes);
    return [...new Uint8Array(hash)].map((value) => value.toString(16).padStart(2, "0")).join("");
}

function assertMemoryLimit(memories) {
    for (const memory of memories) {
        if (memory.buffer.byteLength > MAX_MEMORY_BYTES) {
            throw new Error("ASTRA_UI_COMPONENT_WEB_MEMORY_LIMIT");
        }
    }
}

export async function createUiComponentSession(config) {
    if (!config || !(config.bindingsUrl instanceof URL) || !(config.coreArtifacts instanceof Map)) {
        throw new Error("ASTRA_UI_COMPONENT_WEB_CONFIG_INVALID");
    }
    const bindings = await import(config.bindingsUrl.href);
    if (typeof bindings.instantiate !== "function") {
        throw new Error("ASTRA_UI_COMPONENT_WEB_BINDINGS_INVALID");
    }
    const memories = new Set();
    const getCoreModule = async (relativePath) => {
        if (relativePath.includes("/") || relativePath.includes("\\") || relativePath.includes("..")) {
            throw new Error("ASTRA_UI_COMPONENT_WEB_ARTIFACT_PATH_INVALID");
        }
        const evidence = config.coreArtifacts.get(relativePath);
        if (!evidence || !/^sha256:[0-9a-f]{64}$/.test(evidence.sha256)) {
            throw new Error("ASTRA_UI_COMPONENT_WEB_ARTIFACT_UNTRUSTED");
        }
        let bytes;
        if (config.fetchArtifact !== undefined) {
            if (typeof config.fetchArtifact !== "function") {
                throw new Error("ASTRA_UI_COMPONENT_WEB_FETCHER_INVALID");
            }
            bytes = await config.fetchArtifact(relativePath);
        } else {
            const response = await fetch(new URL(relativePath, config.bindingsUrl));
            if (!response.ok) {
                throw new Error("ASTRA_UI_COMPONENT_WEB_ARTIFACT_FETCH_FAILED");
            }
            bytes = await response.arrayBuffer();
        }
        if (!(bytes instanceof ArrayBuffer)) {
            throw new Error("ASTRA_UI_COMPONENT_WEB_ARTIFACT_BYTES_INVALID");
        }
        if (bytes.byteLength !== evidence.byteSize || `sha256:${await sha256(bytes)}` !== evidence.sha256) {
            throw new Error("ASTRA_UI_COMPONENT_WEB_ARTIFACT_HASH_MISMATCH");
        }
        return WebAssembly.compile(bytes);
    };
    const instantiateCore = async (module, imports) => {
        const instance = await WebAssembly.instantiate(module, imports);
        for (const value of Object.values(instance.exports)) {
            if (value instanceof WebAssembly.Memory) {
                memories.add(value);
            }
        }
        assertMemoryLimit(memories);
        return instance;
    };
    const root = await bindings.instantiate(getCoreModule, {}, instantiateCore);
    if (!root || typeof root.invoke !== "function") {
        throw new Error("ASTRA_UI_COMPONENT_WEB_EXPORT_INVALID");
    }
    return {
        invoke(version, sequence, deadlineNs, payloadHash, payload) {
            if (version !== 1 || typeof sequence !== "bigint" || typeof deadlineNs !== "bigint" || deadlineNs <= 0n) {
                throw new Error("ASTRA_UI_COMPONENT_WEB_REQUEST_INVALID");
            }
            if (!(payloadHash instanceof Uint8Array) || payloadHash.length !== HASH_BYTES || !(payload instanceof Uint8Array) || payload.length > MAX_DTO_BYTES) {
                throw new Error("ASTRA_UI_COMPONENT_WEB_REQUEST_LIMIT");
            }
            let response;
            try {
                response = root.invoke(version, sequence, deadlineNs, payloadHash, payload);
            } catch (error) {
                throw new Error("ASTRA_UI_COMPONENT_WEB_TRAP", { cause: error });
            }
            assertMemoryLimit(memories);
            if (!Array.isArray(response) || response.length !== 5 || response[0] !== version || response[1] !== sequence || !(response[2] instanceof Uint8Array) || response[2].length !== HASH_BYTES || !(response[3] instanceof Uint8Array) || response[3].length > MAX_DTO_BYTES) {
                throw new Error("ASTRA_UI_COMPONENT_WEB_RESPONSE_INVALID");
            }
            if (response[4] !== undefined) {
                throw new Error("ASTRA_UI_COMPONENT_WEB_FAILED");
            }
            return response;
        },
    };
}
