package ai.uldren.loom;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;

/**
 * Mail facet operations for a {@link LoomSession}: mailboxes under a principal, with RFC 5322 messages
 * (CAS-stored body + a structured index + flags) keyed by UID. Reached via {@link LoomSession#mail()}.
 * Owns the FFM downcalls directly via {@link Loom#onHandle}.
 *
 * <p>Licensed under BUSL-1.1 (see the workspace {@code LICENSE}). (c) Uldren Technologies LLC.
 */
public final class MailOps {
    private final LoomSession session;

    MailOps(LoomSession session) {
        this.session = session;
    }

    /** Create (or replace the metadata of) mailbox {@code mailbox} under {@code principal}. */
    public void createMailbox(String workspace, String principal, String mailbox, String displayName) {
        session.onHandle("loom_mail_create_mailbox",
                (arena, handle) -> {
                    int status = (int) Loom.LOOM_MAIL_CREATE_MAILBOX.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(mailbox), arena.allocateFrom(displayName));
                    if (status != 0) {
                        throw Loom.lastError("loom_mail_create_mailbox");
                    }
                    return null;
                });
    }

    /** Delete mailbox {@code mailbox} and its messages; returns whether it existed. */
    public boolean deleteMailbox(String workspace, String principal, String mailbox) {
        return session.onHandle("loom_mail_delete_mailbox",
                (arena, handle) -> {
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_MAIL_DELETE_MAILBOX.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(mailbox), outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_mail_delete_mailbox");
                    }
                    return outFound.get(ValueLayout.JAVA_INT, 0) != 0;
                });
    }

    /** The mailbox ids under {@code principal} as Loom Canonical CBOR (sorted). */
    public byte[] listMailboxes(String workspace, String principal) {
        return session.onHandle("loom_mail_list_mailboxes",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_MAIL_LIST_MAILBOXES.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_mail_list_mailboxes");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Ingest the raw RFC 5322 message {@code raw} at {@code uid}; returns the body's content address. */
    public String ingestMessage(String workspace, String principal, String mailbox, String uid,
            byte[] raw) {
        return session.onHandle("loom_mail_ingest_message",
                (arena, handle) -> {
                    MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                    int status = (int) Loom.LOOM_MAIL_INGEST_MESSAGE.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(mailbox), arena.allocateFrom(uid),
                            Loom.bytesOrNull(arena, raw), (long) (raw != null ? raw.length : 0), out);
                    if (status != 0) {
                        throw Loom.lastError("loom_mail_ingest_message");
                    }
                    return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
                });
    }

    /** Fetch the structured index of the message at {@code uid}, or {@code null} if absent. */
    public byte[] getMessage(String workspace, String principal, String mailbox, String uid) {
        return session.onHandle("loom_mail_get_message",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_MAIL_GET_MESSAGE.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(mailbox), arena.allocateFrom(uid), outPtr, outLen, outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_mail_get_message");
                    }
                    if (outFound.get(ValueLayout.JAVA_INT, 0) == 0) {
                        return null;
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Fetch the raw RFC 5322 body (.eml bytes) of the message at {@code uid}, or {@code null} if absent. */
    public byte[] toEml(String workspace, String principal, String mailbox, String uid) {
        return session.onHandle("loom_mail_to_eml",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_MAIL_GET_BODY.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(mailbox), arena.allocateFrom(uid), outPtr, outLen, outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_mail_to_eml");
                    }
                    if (outFound.get(ValueLayout.JAVA_INT, 0) == 0) {
                        return null;
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Remove the message index and its flags at {@code uid}; returns whether it was present. */
    public boolean deleteMessage(String workspace, String principal, String mailbox, String uid) {
        return session.onHandle("loom_mail_delete_message",
                (arena, handle) -> {
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_MAIL_DELETE_MESSAGE.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(mailbox), arena.allocateFrom(uid), outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_mail_delete_message");
                    }
                    return outFound.get(ValueLayout.JAVA_INT, 0) != 0;
                });
    }

    /** List {@code mailbox} as a Loom Canonical CBOR array of per-message {@code MailMessage} CBOR. */
    public byte[] listMessages(String workspace, String principal, String mailbox) {
        return session.onHandle("loom_mail_list_messages",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_MAIL_LIST_MESSAGES.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(mailbox), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_mail_list_messages");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** The flags/labels on the message at {@code uid} as a Loom Canonical CBOR text array. */
    public byte[] getFlags(String workspace, String principal, String mailbox, String uid) {
        return session.onHandle("loom_mail_get_flags",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_MAIL_GET_FLAGS.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(mailbox), arena.allocateFrom(uid), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_mail_get_flags");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Replace the flags/labels on the message at {@code uid} with {@code flags} (CBOR text array). */
    public void setFlags(String workspace, String principal, String mailbox, String uid, byte[] flags) {
        session.onHandle("loom_mail_set_flags",
                (arena, handle) -> {
                    int status = (int) Loom.LOOM_MAIL_SET_FLAGS.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(mailbox), arena.allocateFrom(uid),
                            Loom.bytesOrNull(arena, flags), (long) (flags != null ? flags.length : 0));
                    if (status != 0) {
                        throw Loom.lastError("loom_mail_set_flags");
                    }
                    return null;
                });
    }

    /** Case-insensitive substring search over {@code mailbox} by subject/from, as CBOR. */
    public byte[] search(String workspace, String principal, String mailbox, String text) {
        return session.onHandle("loom_mail_search",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_MAIL_SEARCH.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(mailbox), arena.allocateFrom(text), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_mail_search");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }
}
