#!/bin/sh
set -eu
# Keep stock TLS enabled: v0.6.0 Electrum requires it at startup. The test-only
# forwarder accepts the node's generated certificate on container loopback.
# RPC authentication and response bytes still come from the stock node.
socat TCP-LISTEN:18334,bind=0.0.0.0,reuseaddr,fork OPENSSL:127.0.0.1:18333,verify=0 &
# Stock regtest selects only loopback sync peers. Forward P2P bytes unchanged.
if [ -n "${REGTEST_PEER:-}" ]; then
    socat TCP-LISTEN:18445,bind=127.0.0.1,reuseaddr,fork "TCP:${REGTEST_PEER}:18444" &
    set -- "$@" --connect=127.0.0.1:18445
fi
exec utreexod --regtest --rpclisten=127.0.0.1:18333 \
    --rpcuser=test --rpcpass=test --datadir=/data --logdir=/data/logs \
    --nodnsseed --noonion --prune=0 --noassumeutreexo \
    --listen=0.0.0.0:18444 "$@"
