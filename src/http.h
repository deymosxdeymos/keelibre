#include <stddef.h>

/*
 * http.h — minimal localhost HTTP server for the keebyd switch-picker UI.
 */
#ifndef KEEBY_HTTP_H
#define KEEBY_HTTP_H

typedef struct HttpRequest {
    char   method[8];
    char   path[256];
    char   query[512];
    char  *body;        /* may be NULL */
    size_t body_len;
} HttpRequest;

typedef void (*HttpHandler)(const HttpRequest *req, int client_fd);

/* Start a background thread serving on 127.0.0.1:port. handler is called for
 * every request; it must write a full HTTP response to client_fd. */
int http_server_start(int port, HttpHandler handler);
void http_server_stop(void);

/* response helpers */
void http_respond(int fd, const char *status, const char *content_type,
                  const void *body, size_t len);
void respond_json(int fd, const char *json);
size_t urldecode(char *s);
char *query_get(char *query, const char *key);

#endif
