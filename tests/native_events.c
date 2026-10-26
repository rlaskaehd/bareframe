#include <assert.h>
#include <stdio.h>
#include "bareframe_events.h"

int main(void) {
    _Static_assert(sizeof(bareframe_host_event) == 4112, "Rust FFI layout must match C");
    bareframe_event_queue queue = {0};
    bareframe_host_event event;
    assert(!bareframe_event_pop(&queue, &event));
    bareframe_event_push(&queue, 1, "한글 title", -1, 0);
    bareframe_event_push(&queue, 3, NULL, 7, 123456789);
    assert(bareframe_event_pop(&queue, &event));
    assert(event.kind == 1 && strcmp(event.text, "한글 title") == 0);
    assert(bareframe_event_pop(&queue, &event));
    assert(event.kind == 3 && event.exit_code == 7 && event.duration == 123456789);
    for (int i = 0; i < 100; i++) bareframe_event_push(&queue, 3, NULL, i, 0);
    assert(bareframe_event_pop(&queue, &event));
    assert(event.kind == 4 && event.duration == 96);
    for (int i = 96; i < 100; i++) {
        assert(bareframe_event_pop(&queue, &event));
        assert(event.kind == 3 && event.exit_code == i);
    }
    char oversized[4097];
    memset(oversized, 'x', sizeof(oversized) - 1);
    oversized[sizeof(oversized) - 1] = 0;
    bareframe_event_push(&queue, 1, "before oversized metadata", -1, 0);
    bareframe_event_push(&queue, 2, oversized, -1, 0);
    assert(bareframe_event_pop(&queue, &event));
    assert(event.kind == 4 && event.duration == 2);
    assert(!bareframe_event_pop(&queue, &event));
    puts("native observation queue: FIFO, UTF-8, command status, overflow and oversized metadata OK");
}
