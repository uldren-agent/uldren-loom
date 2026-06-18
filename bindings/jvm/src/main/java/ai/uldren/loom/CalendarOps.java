package ai.uldren.loom;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;

/**
 * Calendar facet operations for a {@link LoomSession}: CalDAV-style collections under a principal, with
 * typed iCalendar entries keyed by UID. Reached via {@link LoomSession#calendar()}. Entries and listing
 * results cross as Loom Canonical CBOR; {@code entryIcs}/{@code putIcs} are the on-demand iCalendar
 * (`.ics`) projection. Owns the FFM downcalls directly via {@link Loom#onHandle}.
 *
 * <p>Licensed under BUSL-1.1 (see the workspace {@code LICENSE}). (c) Uldren Technologies LLC.
 */
public final class CalendarOps {
    private final LoomSession session;

    CalendarOps(LoomSession session) {
        this.session = session;
    }

    /** Create (or replace the metadata of) collection {@code collection} under {@code principal}. */
    public void createCollection(String workspace, String principal, String collection,
            String displayName, String components) {
        session.onHandle("loom_cal_create_collection",
                (arena, handle) -> {
                    int status = (int) Loom.LOOM_CAL_CREATE_COLLECTION.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(collection), arena.allocateFrom(displayName),
                            arena.allocateFrom(components));
                    if (status != 0) {
                        throw Loom.lastError("loom_cal_create_collection");
                    }
                    return null;
                });
    }

    /** Delete collection {@code collection} and its entries; returns whether it existed. */
    public boolean deleteCollection(String workspace, String principal, String collection) {
        return session.onHandle("loom_cal_delete_collection",
                (arena, handle) -> {
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_CAL_DELETE_COLLECTION.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(collection), outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_cal_delete_collection");
                    }
                    return outFound.get(ValueLayout.JAVA_INT, 0) != 0;
                });
    }

    /** The collection ids under {@code principal} as Loom Canonical CBOR (sorted). */
    public byte[] listCollections(String workspace, String principal) {
        return session.onHandle("loom_cal_list_collections",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_CAL_LIST_COLLECTIONS.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_cal_list_collections");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Put the {@code CalendarEntry} CBOR {@code entry} into {@code collection}. */
    public void putEntry(String workspace, String principal, String collection, byte[] entry) {
        session.onHandle("loom_cal_put_entry",
                (arena, handle) -> {
                    int status = (int) Loom.LOOM_CAL_PUT_ENTRY.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(collection), Loom.bytesOrNull(arena, entry),
                            (long) (entry != null ? entry.length : 0));
                    if (status != 0) {
                        throw Loom.lastError("loom_cal_put_entry");
                    }
                    return null;
                });
    }

    /** Fetch the entry at {@code uid} as {@code CalendarEntry} CBOR, or {@code null} if absent. */
    public byte[] getEntry(String workspace, String principal, String collection, String uid) {
        return session.onHandle("loom_cal_get_entry",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_CAL_GET_ENTRY.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(collection), arena.allocateFrom(uid), outPtr, outLen,
                            outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_cal_get_entry");
                    }
                    if (outFound.get(ValueLayout.JAVA_INT, 0) == 0) {
                        return null;
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Remove the entry at {@code uid}; returns whether it was present. */
    public boolean deleteEntry(String workspace, String principal, String collection, String uid) {
        return session.onHandle("loom_cal_delete_entry",
                (arena, handle) -> {
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_CAL_DELETE_ENTRY.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(collection), arena.allocateFrom(uid), outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_cal_delete_entry");
                    }
                    return outFound.get(ValueLayout.JAVA_INT, 0) != 0;
                });
    }

    /** List the entries of {@code collection} as Loom Canonical CBOR. */
    public byte[] listEntries(String workspace, String principal, String collection) {
        return session.onHandle("loom_cal_list_entries",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_CAL_LIST_ENTRIES.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(collection), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_cal_list_entries");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Entries overlapping the half-open window {@code [from, to)} ({@code YYYYMMDDTHHMMSS}), as CBOR. */
    public byte[] range(String workspace, String principal, String collection, String from, String to) {
        return session.onHandle("loom_cal_range",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_CAL_RANGE.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(collection), arena.allocateFrom(from),
                            arena.allocateFrom(to), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_cal_range");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Case-insensitive substring search within {@code component} over {@code collection}, as CBOR. */
    public byte[] search(String workspace, String principal, String collection, String component,
            String text) {
        return session.onHandle("loom_cal_search",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_CAL_SEARCH.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(collection), arena.allocateFrom(component),
                            arena.allocateFrom(text), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_cal_search");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** The on-demand iCalendar (`.ics`) projection of the entry at {@code uid}, or {@code null}. */
    public String entryIcs(String workspace, String principal, String collection, String uid) {
        return session.onHandle("loom_cal_entry_ics",
                (arena, handle) -> {
                    MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_CAL_ENTRY_ICS.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(collection), arena.allocateFrom(uid), out, outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_cal_entry_ics");
                    }
                    if (outFound.get(ValueLayout.JAVA_INT, 0) == 0) {
                        return null;
                    }
                    return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
                });
    }

    /** Ingest an iCalendar (`.ics`) document into {@code collection}; returns the entry's ETag. */
    public String putIcs(String workspace, String principal, String collection, String ics) {
        return session.onHandle("loom_cal_put_ics",
                (arena, handle) -> {
                    MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                    int status = (int) Loom.LOOM_CAL_PUT_ICS.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(principal),
                            arena.allocateFrom(collection), arena.allocateFrom(ics), out);
                    if (status != 0) {
                        throw Loom.lastError("loom_cal_put_ics");
                    }
                    return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
                });
    }
}
