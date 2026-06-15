import Foundation
import XCTest

@testable import UldrenLoom

final class LoomTests: XCTestCase {
    func testVersionIsNonEmpty() {
        XCTAssertFalse(Loom.version().isEmpty)
    }

    func testBlobDigestShape() {
        // The canonical "abc" vector lives in the `uldren-loom-conformance` crate; here we only
        // assert the address shape so the test needs no hard-coded digest: "blake3:" + 64 hex chars.
        let digest = Loom.blobDigest(Data("abc".utf8))
        XCTAssertTrue(digest.hasPrefix("blake3:"))
        XCTAssertEqual(digest.count, "blake3:".count + 64)
    }
}
