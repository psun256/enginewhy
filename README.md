# nginy
Production't graden't load balancer.

## Quick links

## Todo
- [ ] architecture astronauting
  - balancer module
    - just the algorithms i guess
- 
  - backend module
    - manages the backend pool
    - deals with health / load check
    - BackendPool for all the backends stored together
    - Backend for individual backends
    - has some methods used by balancer module to pick a suitable backend
  - proxy module 
    - all the different supported protocols to handle
    - will create a session / stream context structure (ConnectionContext)
      - not globally tracked (this might change for UDP!)
      - mainly some metadata
  - config module
    - set up all the stuff or something
- [ ] stream / session handling (i think wrapper around tokio TcpStream)
- [ ] basic backend pooling
- [ ] layer 4 load balancing

## notes
tcp, for nginx (and haproxy, its similar):
```c
// nginx
struct ngx_connection_s {
    void               *data;
    ngx_event_t        *read;
    ngx_event_t        *write;

    ngx_socket_t        fd;

    ngx_recv_pt         recv; // fn pointer to whatever recv fn used (different for idfferent platforms / protocol
    ngx_send_pt         send; // ditto
    ngx_recv_chain_pt   recv_chain;
    ngx_send_chain_pt   send_chain;

    ngx_listening_t    *listening;

    off_t               sent;

    ngx_log_t          *log;

    ngx_pool_t         *pool;

    int                 type;

    struct sockaddr    *sockaddr;
    socklen_t           socklen;
    ngx_str_t           addr_text;

    ngx_proxy_protocol_t  *proxy_protocol;

#if (NGX_QUIC || NGX_COMPAT)
    ngx_quic_stream_t     *quic;
#endif

#if (NGX_SSL || NGX_COMPAT)
    ngx_ssl_connection_t  *ssl;
#endif

    ngx_udp_connection_t  *udp; // additional stuff for UDP (which is technically connectionless, but they use timeouts and a rbtree to store "sessions")

    struct sockaddr    *local_sockaddr;
    socklen_t           local_socklen;

    ngx_buf_t          *buffer;

    ngx_queue_t         queue;

    ngx_atomic_uint_t   number;

    ngx_msec_t          start_time;
    ngx_uint_t          requests;

    unsigned            buffered:8;

    unsigned            log_error:3;     /* ngx_connection_log_error_e */

    unsigned            timedout:1;
    unsigned            error:1;
    unsigned            destroyed:1;
    unsigned            pipeline:1;

    unsigned            idle:1;
    unsigned            reusable:1;
    unsigned            close:1;
    unsigned            shared:1;

    unsigned            sendfile:1;
    unsigned            sndlowat:1;
    unsigned            tcp_nodelay:2;   /* ngx_connection_tcp_nodelay_e */
    unsigned            tcp_nopush:2;    /* ngx_connection_tcp_nopush_e */

    unsigned            need_last_buf:1;
    unsigned            need_flush_buf:1;

#if (NGX_HAVE_SENDFILE_NODISKIO || NGX_COMPAT)
    unsigned            busy_count:2;
#endif

#if (NGX_THREADS || NGX_COMPAT)
    ngx_thread_task_t  *sendfile_task;
#endif
};
```
process to load balance:
- accept incoming connection
- create some kind of stream / session object
  - nginx use this to abstract around tcp and udp layers
  - for us we probably don't need as detailed as them, since we have tokio::net, so itll be a wrapper around TcpStream
- ask the load balancing algorithm which server in the pool to route to
- connect to the server
- proxy the data (copy_bidirectional? maybe we want some metrics or logging, so might do manually)
- cleanup when smoeone leavesr or something goes wrong (with TCP, OS / tokio will tell us, with UDP probably just timeout based, and a periodic sweep of all sessions)


### UDP
UDP is connectionless, and i don't think UdpSocket or UdpFramed implement the traits required for tokio copy_bidirectional
but async write and read don't work on just regular datagrams, so probably not possible.

Would require us to implement our own bidirectional copying / proxying, as well as tracking "active" connections.