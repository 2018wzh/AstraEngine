const MAX_DTO_BYTES = 4 * 1024 * 1024;
const HASH_BYTES = 32;

export function invoke(version, sequence, deadlineNs, payloadHash, payload) {
    if (version !== 1) {
        throw new Error("ASTRA_UI_COMPONENT_PROTOCOL_VERSION");
    }
    if (payload.length > MAX_DTO_BYTES) {
        throw new Error("ASTRA_UI_COMPONENT_DTO_LIMIT");
    }
    if (payloadHash.length !== HASH_BYTES) {
        throw new Error("ASTRA_UI_COMPONENT_HASH_LENGTH");
    }
    if (deadlineNs === 0n) {
        throw new Error("ASTRA_UI_COMPONENT_DEADLINE_INVALID");
    }
    return [version, sequence, payloadHash, payload, undefined];
}
