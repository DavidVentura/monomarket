set -euo pipefail
source .env
export FOUNDRY_DISABLE_NIGHTLY_WARNING=1
ADDRESS=$(cast wallet address $PRIVATE_KEY)
cast send $CONTRACT_ADDRESS "buy(uint256)" 1  --private-key $PRIVATE_KEY
