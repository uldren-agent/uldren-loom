package ai.uldren.loom;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;

/**
 * Contacts facet operations for a {@link LoomSession}: address books under a principal, with typed
 * vCard entries keyed by UID. Reached via {@link LoomSession#contacts()}. Entries and listing results
 * cross as Loom Canonical CBOR; {@code entryVcard}/{@code putVcard} are the on-demand vCard (`.vcf`)
 * projection. Owns the FFM downcalls directly via {@link Loom#onHandle}.
 *
 * <p>Licensed under BUSL-1.1 (see the workspace {@code LICENSE}). (c) Uldren Technologies LLC.
 */
public final class ContactsOps {
    private final LoomSession session;

    ContactsOps(LoomSession session) {
        this.session = session;
    }

    /** Create (or replace the metadata of) address book {@code book} under {@code principal}. */
    public void createBook(String workspace, String principal, String book, String displayName) {
        session.onHandle("loom_card_create_book",
                (arena, handle) -> {
                    int status = (int) Loom.LOOM_CARD_CREATE_BOOK.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(book), arena.allocateFrom(displayName));
                    if (status != 0) {
                        throw Loom.lastError("loom_card_create_book");
                    }
                    return null;
                });
    }

    /** Delete address book {@code book} and its contacts; returns whether it existed. */
    public boolean deleteBook(String workspace, String principal, String book) {
        return session.onHandle("loom_card_delete_book",
                (arena, handle) -> {
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_CARD_DELETE_BOOK.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(book), outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_card_delete_book");
                    }
                    return outFound.get(ValueLayout.JAVA_INT, 0) != 0;
                });
    }

    /** The address-book ids under {@code principal} as Loom Canonical CBOR (sorted). */
    public byte[] listBooks(String workspace, String principal) {
        return session.onHandle("loom_card_list_books",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_CARD_LIST_BOOKS.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_card_list_books");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Put the {@code ContactEntry} CBOR {@code entry} into {@code book}. */
    public void putEntry(String workspace, String principal, String book, byte[] entry) {
        session.onHandle("loom_card_put_entry",
                (arena, handle) -> {
                    int status = (int) Loom.LOOM_CARD_PUT_ENTRY.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(book), Loom.bytesOrNull(arena, entry),
                            (long) (entry != null ? entry.length : 0));
                    if (status != 0) {
                        throw Loom.lastError("loom_card_put_entry");
                    }
                    return null;
                });
    }

    /** Fetch the contact at {@code uid} as {@code ContactEntry} CBOR, or {@code null} if absent. */
    public byte[] getEntry(String workspace, String principal, String book, String uid) {
        return session.onHandle("loom_card_get_entry",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_CARD_GET_ENTRY.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(book), arena.allocateFrom(uid), outPtr, outLen, outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_card_get_entry");
                    }
                    if (outFound.get(ValueLayout.JAVA_INT, 0) == 0) {
                        return null;
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Remove the contact at {@code uid}; returns whether it was present. */
    public boolean deleteEntry(String workspace, String principal, String book, String uid) {
        return session.onHandle("loom_card_delete_entry",
                (arena, handle) -> {
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_CARD_DELETE_ENTRY.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(book), arena.allocateFrom(uid), outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_card_delete_entry");
                    }
                    return outFound.get(ValueLayout.JAVA_INT, 0) != 0;
                });
    }

    /** List the contacts of {@code book} as Loom Canonical CBOR. */
    public byte[] listEntries(String workspace, String principal, String book) {
        return session.onHandle("loom_card_list_entries",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_CARD_LIST_ENTRIES.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(book), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_card_list_entries");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Case-insensitive substring search over {@code book} by formatted name/org/email, as CBOR. */
    public byte[] search(String workspace, String principal, String book, String text) {
        return session.onHandle("loom_card_search",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_CARD_SEARCH.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(book), arena.allocateFrom(text), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_card_search");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** The on-demand vCard (`.vcf`) projection of the contact at {@code uid}, or {@code null}. */
    public String entryVcard(String workspace, String principal, String book, String uid) {
        return session.onHandle("loom_card_entry_vcard",
                (arena, handle) -> {
                    MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_CARD_ENTRY_VCARD.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(book), arena.allocateFrom(uid), out, outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_card_entry_vcard");
                    }
                    if (outFound.get(ValueLayout.JAVA_INT, 0) == 0) {
                        return null;
                    }
                    return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
                });
    }

    /** Ingest a vCard (`.vcf`) document into {@code book}; returns the contact's ETag. */
    public String putVcard(String workspace, String principal, String book, String vcf) {
        return session.onHandle("loom_card_put_vcard",
                (arena, handle) -> {
                    MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                    int status = (int) Loom.LOOM_CARD_PUT_VCARD.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(book), arena.allocateFrom(vcf), out);
                    if (status != 0) {
                        throw Loom.lastError("loom_card_put_vcard");
                    }
                    return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
                });
    }
}
