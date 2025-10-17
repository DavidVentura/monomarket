set -euo pipefail
source .env
export FOUNDRY_DISABLE_NIGHTLY_WARNING=1
ADDRESS=$(cast wallet address $PRIVATE_KEY)
#90s
#cast send $CONTRACT_ADDRESS "start(uint256)" 200  --private-key $PRIVATE_KEY;
#9s
cast send $CONTRACT_ADDRESS "start(uint256)" 20  --private-key $PRIVATE_KEY;
