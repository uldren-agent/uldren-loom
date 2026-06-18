import CUldrenLoom
import Foundation

public extension Loom {
    func lanesCreate(workspace: String, lane: [UInt8]) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = lane.withUnsafeBufferPointer { buf in
            loom_lanes_create_cbor(session, workspace, buf.baseAddress, UInt(buf.count), &ptr, &len)
        }
        guard status == 0 else { throw LoomSql.lastError() }
        return Loom.takeBytes(ptr, len)
    }

    func lanesGet(workspace: String, laneId: String) throws -> [UInt8]? {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        var found: Int32 = 0
        let status = loom_lanes_get_cbor(session, workspace, laneId, &ptr, &len, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        guard found != 0 else { return nil }
        return Loom.takeBytes(ptr, len)
    }

    func lanesList(workspace: String) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_lanes_list_cbor(session, workspace, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        return Loom.takeBytes(ptr, len)
    }

    func lanesUpdate(workspace: String, laneId: String, title: String? = nil,
                     description: String? = nil, laneStatus: String? = nil,
                     statusReport: String? = nil, reviewerFeedback: String? = nil,
                     updatedBy: String) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_lanes_update_cbor(session, workspace, laneId, title, description,
                                            laneStatus, statusReport, reviewerFeedback,
                                            updatedBy, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        return Loom.takeBytes(ptr, len)
    }

    func lanesTicketAdd(workspace: String, laneId: String, ticketId: String,
                        updatedBy: String, placement: String = "append",
                        anchor: String? = nil) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_lanes_ticket_add_cbor(session, workspace, laneId, ticketId, updatedBy,
                                                placement, anchor, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        return Loom.takeBytes(ptr, len)
    }

    func lanesTicketRemove(workspace: String, laneId: String, ticketId: String,
                           updatedBy: String) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_lanes_ticket_remove_cbor(session, workspace, laneId, ticketId, updatedBy,
                                                   &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        return Loom.takeBytes(ptr, len)
    }

}
