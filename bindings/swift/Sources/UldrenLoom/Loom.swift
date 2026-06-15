import CUldrenLoom
import Foundation

/// Swift binding over the Uldren Loom C ABI (`libuldren_loom`).
///
/// Every string returned by the C ABI is owned by the library and freed here with
/// `loom_string_free`, matching the ownership contract in `include/loom.h`.
public enum Loom {
    /// The engine version (the crate's `CARGO_PKG_VERSION`).
    public static func version() -> String {
        guard let ptr = loom_version() else { return "" }
        defer { loom_string_free(ptr) }
        return String(cString: ptr)
    }

    /// The content address (`"algo:hex"`, e.g. `blake3:...`) of `data` as an Uldren Loom blob.
    public static func blobDigest(_ data: Data) -> String {
        data.withUnsafeBytes { raw -> String in
            let base = raw.bindMemory(to: UInt8.self).baseAddress
            // The C ABI accepts a null pointer when len == 0.
            guard let ptr = loom_blob_digest(base, UInt(raw.count)) else { return "" }
            defer { loom_string_free(ptr) }
            return String(cString: ptr)
        }
    }
}
