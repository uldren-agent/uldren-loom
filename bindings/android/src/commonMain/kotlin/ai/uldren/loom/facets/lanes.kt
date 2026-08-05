package ai.uldren.loom

enum class LaneTicketPlacement(val abiValue: Int) {
    FIRST(1),
    LAST(2),
    BEFORE(3),
    AFTER(4),
}

expect fun Loom.lanesCreate(path: String, workspace: String, lane: ByteArray, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): ByteArray
expect fun Loom.lanesGet(path: String, workspace: String, laneId: String, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): ByteArray?
expect fun Loom.lanesList(path: String, workspace: String, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): ByteArray
expect fun Loom.lanesUpdate(path: String, workspace: String, laneId: String, title: String? = null, description: String? = null, laneStatus: String? = null, statusReport: String? = null, reviewerFeedback: String? = null, updatedBy: String, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): ByteArray
expect fun Loom.lanesTicketAdd(path: String, workspace: String, laneId: String, ticketId: String, updatedBy: String, placement: LaneTicketPlacement = LaneTicketPlacement.LAST, anchor: String? = null, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): ByteArray
expect fun Loom.lanesTicketRemove(path: String, workspace: String, laneId: String, ticketId: String, updatedBy: String, passphrase: String? = null, kek: ByteArray? = null, authPrincipal: String? = null, authPassphrase: String? = null): ByteArray
