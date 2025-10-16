deploy-contract:
	forge create --private-key $$(cat pkey) contract/src/StockMarket.sol:StockMarket --broadcast
start:
	bash start.sh
