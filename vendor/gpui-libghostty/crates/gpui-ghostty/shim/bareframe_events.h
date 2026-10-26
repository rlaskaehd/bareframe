#ifndef BAREFRAME_EVENTS_H
#define BAREFRAME_EVENTS_H
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <string.h>

typedef struct {
    int32_t kind;
    int32_t exit_code;
    uint64_t duration;
    char text[4096];
} bareframe_host_event;

#define BAREFRAME_EVENT_CAPACITY 32
typedef struct {
    bareframe_host_event events[BAREFRAME_EVENT_CAPACITY];
    size_t head;
    size_t count;
    uint64_t dropped;
} bareframe_event_queue;

// The caller serializes access. Never silently truncate command metadata.
static inline void bareframe_event_push(bareframe_event_queue *queue, int32_t kind,
                                       const char *text, int32_t code, uint64_t duration) {
    size_t length = text == NULL ? 0 : strlen(text);
    if (length >= sizeof(queue->events[0].text)) {
        queue->dropped += queue->count + 1;
        queue->count = 0;
        queue->head = 0;
        return;
    }
    if (queue->count == BAREFRAME_EVENT_CAPACITY) {
        queue->dropped += queue->count;
        queue->count = 0;
        queue->head = 0;
    }
    size_t index = (queue->head + queue->count) % BAREFRAME_EVENT_CAPACITY;
    bareframe_host_event *event = &queue->events[index];
    *event = (bareframe_host_event){ .kind = kind, .exit_code = code, .duration = duration };
    if (text != NULL) memcpy(event->text, text, length);
    event->text[length] = 0;
    queue->count++;
}

static inline bool bareframe_event_pop(bareframe_event_queue *queue, bareframe_host_event *output) {
    if (queue->dropped != 0) {
        *output = (bareframe_host_event){ .kind = 4, .exit_code = -1, .duration = queue->dropped };
        queue->dropped = 0;
        return true;
    }
    if (queue->count == 0) return false;
    *output = queue->events[queue->head];
    queue->head = (queue->head + 1) % BAREFRAME_EVENT_CAPACITY;
    queue->count--;
    return true;
}
#endif
