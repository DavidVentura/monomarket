set -euo pipefail
source .env
export FOUNDRY_DISABLE_NIGHTLY_WARNING=1
ADDRESS=$(cast wallet address $PRIVATE_KEY)
NONCE=$(cast nonce $ADDRESS)
cast send $CONTRACT_ADDRESS "tick()"  --private-key $PRIVATE_KEY --nonce $NONCE;
((NONCE++))
while true; do
	cast send $CONTRACT_ADDRESS "tick()"  --private-key $PRIVATE_KEY --async --nonce $NONCE  --gas-limit 35655;
	echo "Sent tx with nonce $NONCE at $(date +%s%3N)ms"
	((NONCE++))
	sleep 0.3;
done
