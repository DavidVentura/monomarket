// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

contract StockMarket {
    uint256 public price = 50;
    uint256 public lastTickBlock;

    uint256 public constant INITIAL_CREDITS = 1000;
    uint256 public constant MIN_PRICE = 0;
    uint256 public constant MAX_PRICE = 100;

    mapping(address => uint256) public balances;
    mapping(address => uint256) public holdings;
    mapping(address => bool) public isActive;
    address[] public activeAddresses;

    event PriceUpdate(uint256 oldPrice, uint256 newPrice, uint256 blockNumber);
    event Bought(address indexed user, uint256 amount, uint256 price, uint256 timestamp, uint256 blockNumber, uint256 newBalance, uint256 newHoldings);
    event Sold(address indexed user, uint256 amount, uint256 price, uint256 timestamp, uint256 blockNumber, uint256 newBalance, uint256 newHoldings);
    event NewUser(address indexed user);

    modifier initializeUser() {
        _initializeUser();
        _;
    }

    function _initializeUser() internal {
        if (!isActive[msg.sender]) {
            balances[msg.sender] = INITIAL_CREDITS;
            isActive[msg.sender] = true;
            activeAddresses.push(msg.sender);
            emit NewUser(msg.sender);
        }
    }

    function register() external {
        require(!isActive[msg.sender], "Already registered");
        balances[msg.sender] = INITIAL_CREDITS;
        isActive[msg.sender] = true;
        activeAddresses.push(msg.sender);
        emit NewUser(msg.sender);
    }

    function tick() external {
        if (lastTickBlock >= block.number) {
            return;
        }
        lastTickBlock = block.number;

        uint256 randomSeed = uint256(keccak256(abi.encodePacked(
            block.timestamp,
            block.prevrandao,
            price,
            msg.sender
        )));

        // forge-lint: disable-next-line(unsafe-typecast)
        int256 changePercent = int256(randomSeed % 21) - 10;

        uint256 oldPrice = price;
        // forge-lint: disable-next-line(unsafe-typecast)
        int256 newPriceInt = int256(price) + (int256(price) * changePercent) / 100;

        // forge-lint: disable-next-line(unsafe-typecast)
        if (newPriceInt < int256(MIN_PRICE)) {
            // forge-lint: disable-next-line(unsafe-typecast)
            newPriceInt = int256(MIN_PRICE);
        }
        // forge-lint: disable-next-line(unsafe-typecast)
        if (newPriceInt > int256(MAX_PRICE)) {
            // forge-lint: disable-next-line(unsafe-typecast)
            newPriceInt = int256(MAX_PRICE);
        }

        // forge-lint: disable-next-line(unsafe-typecast)
        price = uint256(newPriceInt);
        emit PriceUpdate(oldPrice, price, block.number);
    }

    function buy(uint256 amount) external {
        require(isActive[msg.sender], "Not registered");
        require(amount > 0, "Amount must be greater than 0");

        uint256 cost = amount * price;
        require(balances[msg.sender] >= cost, "Insufficient credits");

        balances[msg.sender] -= cost;
        holdings[msg.sender] += amount;

        emit Bought(msg.sender, amount, price, block.timestamp, block.number, balances[msg.sender], holdings[msg.sender]);
    }

    function sell(uint256 amount) external {
        require(isActive[msg.sender], "Not registered");
        require(amount > 0, "Amount must be greater than 0");
        require(holdings[msg.sender] >= amount, "Insufficient holdings");

        uint256 revenue = amount * price;

        holdings[msg.sender] -= amount;
        balances[msg.sender] += revenue;

        emit Sold(msg.sender, amount, price, block.timestamp, block.number, balances[msg.sender], holdings[msg.sender]);
    }

    function getBalance(address user) external view returns (uint256) {
        return balances[user];
    }

    function getHoldings(address user) external view returns (uint256) {
        return holdings[user];
    }

    function getNetWorth(address user) external view returns (uint256) {
        return holdings[user] * price + balances[user];
    }

    function getAllActiveAddresses() external view returns (address[] memory) {
        return activeAddresses;
    }

    function getActiveAddressCount() external view returns (uint256) {
        return activeAddresses.length;
    }
}
