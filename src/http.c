/*
 * http.c — tiny blocking HTTP/1.1 server on its own thread (localhost only).
 */
#include "http.h"

#include <arpa/inet.h>
#include <ctype.h>
#include <errno.h>
#include <netinet/in.h>
#include <pthread.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static HttpHandler g_handler;
static volatile int g_listen_fd = -1;
static volatile int g_running;
static pthread_t g_thread;

void http_respond(int fd, const char *status, const char *content_type,
                  const void *body, size_t len)
{
    char hdr[512];
    int n = snprintf(hdr, sizeof(hdr),
                     "HTTP/1.1 %s\r\n"
                     "Content-Type: %s\r\n"
                     "Content-Length: %zu\r\n"
                     "Cache-Control: no-cache\r\n"
                     "Access-Control-Allow-Origin: *\r\n"
                     "Connection: close\r\n\r\n",
                     status, content_type, len);
    (void)!write(fd, hdr, n);
    if (body && len) (void)!write(fd, body, len);
}

void respond_json(int fd, const char *json)
{
    http_respond(fd, "200 OK", "application/json", json, strlen(json));
}

/* urldecode in place, returns len */
size_t urldecode(char *s)
{
    char *w = s;
    for (char *p = s; *p; p++) {
        if (*p == '%' && isxdigit((unsigned char)p[1]) && isxdigit((unsigned char)p[2])) {
            char hex[3] = { p[1], p[2], 0 };
            *w++ = (char)strtol(hex, NULL, 16);
            p += 2;
        } else if (*p == '+') {
            *w++ = ' ';
        } else {
            *w++ = *p;
        }
    }
    *w = 0;
    return (size_t)(w - s);
}

/* find ?key=value in query; returns NULL or value (decoded in place) */
char *query_get(char *query, const char *key)
{
    size_t klen = strlen(key);
    char *p = query;
    while (*p) {
        char *amp = strchr(p, '&');
        size_t seglen = amp ? (size_t)(amp - p) : strlen(p);
        if (seglen > klen && !strncmp(p, key, klen) && p[klen] == '=') {
            p[klen] = 0;
            char *v = p + klen + 1;
            p[seglen] = 0;
            urldecode(v);
            return v;
        }
        p = amp ? amp + 1 : p + seglen;
    }
    return NULL;
}

static void handle_client(int fd)
{
    char buf[8192];
    size_t got = 0;
    while (got < sizeof(buf) - 1) {
        ssize_t n = read(fd, buf + got, sizeof(buf) - 1 - got);
        if (n <= 0) break;
        got += (size_t)n;
        if (strstr(buf, "\r\n\r\n")) break;
    }
    buf[got] = 0;

    HttpRequest req = { .body = NULL };
    char *line_end = strstr(buf, "\r\n");
    if (!line_end) return;
    *line_end = 0;
    char *qmark = NULL;
    if (sscanf(buf, "%7s %255s", req.method, req.path) != 2) return;
    qmark = strchr(req.path, '?');
    if (qmark) {
        snprintf(req.query, sizeof(req.query), "%s", qmark + 1);
        *qmark = 0;
    }

    char *headers_end = strstr(line_end + 2, "\r\n\r\n");
    if (headers_end) {
        const char *cl = strcasestr(line_end + 2, "Content-Length:");
        size_t body_expect = cl ? (size_t)atoi(cl + 15) : 0;
        if (body_expect > 0 && body_expect < 65536) {
            char *body = headers_end + 4;
            size_t have = got - (size_t)(body - buf);
            req.body = malloc(body_expect + 1);
            memcpy(req.body, body, have);
            while (have < body_expect) {
                ssize_t n = read(fd, req.body + have, body_expect - have);
                if (n <= 0) break;
                have += (size_t)n;
            }
            req.body[have] = 0;
            req.body_len = have;
        }
    }

    g_handler(&req, fd);
    free(req.body);
}

static void *server_thread(void *arg)
{
    (void)arg;
    while (g_running) {
        int cfd = accept(g_listen_fd, NULL, NULL);
        if (cfd < 0) {
            if (errno == EINTR) continue;
            break;
        }
        handle_client(cfd);
        close(cfd);
    }
    return NULL;
}

int http_server_start(int port, HttpHandler handler)
{
    signal(SIGPIPE, SIG_IGN);
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) return -1;
    int one = 1;
    setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &one, sizeof(one));
    struct sockaddr_in addr = {
        .sin_family = AF_INET,
        .sin_port = htons((uint16_t)port),
        .sin_addr = { htonl(INADDR_LOOPBACK) },
    };
    if (bind(fd, (struct sockaddr *)&addr, sizeof(addr)) < 0 || listen(fd, 8) < 0) {
        fprintf(stderr, "keebyd: ui: cannot bind 127.0.0.1:%d: %s\n", port, strerror(errno));
        close(fd);
        return -1;
    }
    g_listen_fd = fd;
    g_handler = handler;
    g_running = 1;
    if (pthread_create(&g_thread, NULL, server_thread, NULL) != 0) {
        close(fd);
        return -1;
    }
    fprintf(stderr, "keebyd: ui ready at http://127.0.0.1:%d\n", port);
    return 0;
}

void http_server_stop(void)
{
    g_running = 0;
    if (g_listen_fd >= 0) {
        close(g_listen_fd);
        g_listen_fd = -1;
    }
}
