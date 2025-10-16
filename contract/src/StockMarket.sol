// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

contract StockMarket {
    uint256 public price = 50;
    uint256 public lastTickBlock;

    uint256 public constant INITIAL_CREDITS = 1000;
    uint256 public constant MIN_PRICE = 1;
    uint256 public constant MAX_PRICE = 100;

    struct UserData {
        uint128 balance;
        uint128 holdings;
        bool isActive;
    }

    mapping(address => UserData) public userData;
    address[] public activeAddresses;

    event PriceUpdate(uint256 newPrice, uint256 blockNumber);
    event Position(address indexed user, uint256 balance, uint256 holdings, uint256 blockNumber);
    event NewUser(address indexed user);

    modifier initializeUser() {
        _initializeUser();
        _;
    }

    function _initializeUser() internal {
        UserData storage user = userData[msg.sender];
        if (!user.isActive) {
            // forge-lint: disable-next-line(unsafe-typecast)
            user.balance = uint128(INITIAL_CREDITS);
            user.isActive = true;
            activeAddresses.push(msg.sender);
            emit NewUser(msg.sender);
        }
    }

    function register() external {
        UserData storage user = userData[msg.sender];
        if (!user.isActive) {
            // forge-lint: disable-next-line(unsafe-typecast)
            user.balance = uint128(INITIAL_CREDITS);
            user.isActive = true;
            activeAddresses.push(msg.sender);
        }
        emit NewUser(msg.sender);
    }

    function tick() external {
        require(lastTickBlock < block.number, "Already ticked this block");
        lastTickBlock = block.number;

        uint256 randomSeed = uint256(keccak256(abi.encodePacked(
            block.timestamp,
            block.prevrandao,
            price,
            msg.sender
        )));

        // forge-lint: disable-next-line(unsafe-typecast)
        int256 change = int256(randomSeed % 21) - 10;

        // forge-lint: disable-next-line(unsafe-typecast)
        int256 newPriceInt = int256(price) + change;

        // forge-lint: disable-next-line(unsafe-typecast)
        if (newPriceInt < int256(MIN_PRICE)) {
            // forge-lint: disable-next-line(unsafe-typecast)
            newPriceInt = -newPriceInt;
        }
        // forge-lint: disable-next-line(unsafe-typecast)
        if (newPriceInt > int256(MAX_PRICE)) {
            // forge-lint: disable-next-line(unsafe-typecast)
            newPriceInt = int256(MAX_PRICE) * 2 - newPriceInt;
        }

        // forge-lint: disable-next-line(unsafe-typecast)
        price = uint256(newPriceInt);
        emit PriceUpdate(price, block.number);
    }

    function buy(uint256 amount) external {
        UserData storage user = userData[msg.sender];
        require(user.isActive, "Not registered");
        require(amount > 0, "Amount must be greater than 0");

        uint256 currentPrice = price;
        uint256 cost = amount * currentPrice;
        require(user.balance >= cost, "Insufficient credits");

        unchecked {
            // forge-lint: disable-next-line(unsafe-typecast)
            user.balance -= uint128(cost);
            // forge-lint: disable-next-line(unsafe-typecast)
            user.holdings += uint128(amount);
        }

        emit Position(msg.sender, user.balance, user.holdings, block.number);
    }

    function sell(uint256 amount) external {
        UserData storage user = userData[msg.sender];
        require(user.isActive, "Not registered");
        require(amount > 0, "Amount must be greater than 0");
        require(user.holdings >= amount, "Insufficient holdings");

        uint256 currentPrice = price;
        uint256 revenue = amount * currentPrice;

        unchecked {
            // forge-lint: disable-next-line(unsafe-typecast)
            user.holdings -= uint128(amount);
            // forge-lint: disable-next-line(unsafe-typecast)
            user.balance += uint128(revenue);
        }

        emit Position(msg.sender, user.balance, user.holdings, block.number);
    }

    function getBalance(address user) external view returns (uint256) {
        return userData[user].balance;
    }

    function getHoldings(address user) external view returns (uint256) {
        return userData[user].holdings;
    }

    function getNetWorth(address user) external view returns (uint256) {
        UserData storage data = userData[user];
        return uint256(data.holdings) * price + uint256(data.balance);
    }

    function getAllActiveAddresses() external view returns (address[] memory) {
        return activeAddresses;
    }

    function getActiveAddressCount() external view returns (uint256) {
        return activeAddresses.length;
    }
}
