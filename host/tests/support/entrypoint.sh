#!/bin/sh
set -eu
# Stock RPC permits plaintext only on loopback. Forward bytes within this test
# container; the node still authenticates every request. No response rewriting.
socat TCP-LISTEN:18334,bind=0.0.0.0,reuseaddr,fork TCP:127.0.0.1:18333 &
# Stock regtest selects only loopback sync peers. Forward P2P bytes unchanged.
if [ -n "${REGTEST_PEER:-}" ]; then
    socat TCP-LISTEN:18445,bind=127.0.0.1,reuseaddr,fork "TCP:${REGTEST_PEER}:18444" &
    set -- "$@" --connect=127.0.0.1:18445
fi
exec utreexod --regtest --notls --rpclisten=127.0.0.1:18333 \
    --rpcuser=test --rpcpass=test --datadir=/data --logdir=/data/logs \
    --nodnsseed --noonion --prune=0 --noassumeutreexo \
    --listen=0.0.0.0:18444 "$@"
