use crate::data::types::TradeSide;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct Position {
    pub symbol: String,
    pub quantity: Decimal,
    pub avg_price: Decimal,
    pub market_value: Decimal,
    pub unrealized_pnl: Decimal,
}

#[derive(Debug, Clone)]
pub struct Trade {
    pub symbol: String,
    pub side: TradeSide,
    pub quantity: Decimal,
    pub price: Decimal,
    pub timestamp: DateTime<Utc>,
    pub realized_pnl: Option<Decimal>,
    pub commission: Decimal,
}

pub struct Portfolio {
    pub initial_capital: Decimal,
    pub cash: Decimal,
    /// Cash locked for pending buy orders
    pub locked_cash: Decimal,
    pub positions: HashMap<String, Position>,
    pub trades: Vec<Trade>,
    pub current_prices: HashMap<String, Decimal>,
    pub commission_rate: Decimal, // e.g., 0.001 for 0.1%
}

impl Portfolio {
    pub fn new(initial_capital: Decimal) -> Self {
        Self {
            initial_capital,
            cash: initial_capital,
            locked_cash: Decimal::ZERO,
            positions: HashMap::new(),
            trades: Vec::new(),
            current_prices: HashMap::new(),
            commission_rate: Decimal::from_str("0.001").unwrap_or(Decimal::ZERO), // 0.1% default
        }
    }

    pub fn with_commission_rate(mut self, rate: Decimal) -> Self {
        self.commission_rate = rate;
        self
    }

    // === BALANCE MANAGEMENT ===

    /// Get available cash (total cash minus locked for pending orders).
    pub fn available_cash(&self) -> Decimal {
        self.cash - self.locked_cash
    }

    /// Lock funds for a pending buy order.
    ///
    /// This reserves funds so they can't be used by other orders.
    /// Locked funds are released when the order fills or is canceled.
    pub fn lock_funds(&mut self, amount: Decimal) -> Result<(), String> {
        let available = self.available_cash();
        if amount > available {
            return Err(format!(
                "Insufficient available funds: need ${}, available ${}",
                amount, available
            ));
        }
        self.locked_cash += amount;
        Ok(())
    }

    /// Unlock funds when an order is canceled.
    ///
    /// Returns the funds to available cash.
    pub fn unlock_funds(&mut self, amount: Decimal) {
        self.locked_cash = (self.locked_cash - amount).max(Decimal::ZERO);
    }

    /// Release locked funds when an order fills.
    ///
    /// The funds are moved from locked to spent (reduces both locked_cash and cash).
    /// Use this before execute_buy_with_locked_funds.
    pub fn release_locked_funds(&mut self, amount: Decimal) {
        // Just unlock - the actual cash reduction happens in execute_buy
        self.locked_cash = (self.locked_cash - amount).max(Decimal::ZERO);
    }

    /// Check if there's enough available cash (excluding locked funds).
    pub fn has_available_funds(&self, amount: Decimal) -> bool {
        self.available_cash() >= amount
    }

    pub fn update_price(&mut self, symbol: &str, price: Decimal) {
        self.current_prices.insert(symbol.to_string(), price);

        // Update position market value and unrealized PnL
        if let Some(position) = self.positions.get_mut(symbol) {
            position.market_value = position.quantity * price;
            position.unrealized_pnl = (price - position.avg_price) * position.quantity;
        }
    }

    pub fn execute_buy(
        &mut self,
        symbol: String,
        quantity: Decimal,
        price: Decimal,
    ) -> Result<(), String> {
        let cost = quantity * price;
        let commission = cost * self.commission_rate;
        self.execute_buy_with_commission(symbol, quantity, price, commission)
    }

    /// Execute a buy with explicit commission (used by exchange-based execution).
    ///
    /// This method allows external systems (like SimulatedExchange) to provide
    /// the exact commission calculated by their fee model, avoiding double-counting.
    pub fn execute_buy_with_commission(
        &mut self,
        symbol: String,
        quantity: Decimal,
        price: Decimal,
        commission: Decimal,
    ) -> Result<(), String> {
        let cost = quantity * price;
        let total_cost = cost + commission;

        if total_cost > self.cash {
            return Err(format!(
                "Insufficient funds: need ${}, available ${}",
                total_cost, self.cash
            ));
        }

        self.cash -= total_cost;

        match self.positions.get_mut(&symbol) {
            Some(position) => {
                let total_quantity = position.quantity + quantity;
                let total_cost = position.quantity * position.avg_price + cost;
                position.avg_price = total_cost / total_quantity;
                position.quantity = total_quantity;
                position.market_value = total_quantity * price;
                position.unrealized_pnl = (price - position.avg_price) * total_quantity;
            }
            None => {
                self.positions.insert(
                    symbol.clone(),
                    Position {
                        symbol: symbol.clone(),
                        quantity,
                        avg_price: price,
                        market_value: quantity * price,
                        unrealized_pnl: Decimal::ZERO,
                    },
                );
            }
        }

        self.trades.push(Trade {
            symbol,
            side: TradeSide::Buy,
            quantity,
            price,
            timestamp: Utc::now(),
            realized_pnl: None,
            commission,
        });

        Ok(())
    }

    /// Execute a buy using previously locked funds.
    ///
    /// This method should be used when funds were locked at order submission time
    /// (via `lock_funds`) and the order has now filled. The locked funds are released
    /// and the actual cash is deducted.
    ///
    /// # Arguments
    /// * `symbol` - The symbol to buy
    /// * `quantity` - The quantity to buy
    /// * `price` - The fill price
    /// * `commission` - The commission (from exchange fee model)
    /// * `locked_amount` - The amount that was locked at submission time
    pub fn execute_buy_with_locked_funds(
        &mut self,
        symbol: String,
        quantity: Decimal,
        price: Decimal,
        commission: Decimal,
        locked_amount: Decimal,
    ) -> Result<(), String> {
        let cost = quantity * price;
        let total_cost = cost + commission;

        // Release the locked funds first
        self.release_locked_funds(locked_amount);

        // Check if we have enough cash (should always pass if locking was done correctly)
        if total_cost > self.cash {
            return Err(format!(
                "Insufficient funds after unlock: need ${}, available ${}",
                total_cost, self.cash
            ));
        }

        self.cash -= total_cost;

        match self.positions.get_mut(&symbol) {
            Some(position) => {
                let total_quantity = position.quantity + quantity;
                let total_cost = position.quantity * position.avg_price + cost;
                position.avg_price = total_cost / total_quantity;
                position.quantity = total_quantity;
                position.market_value = total_quantity * price;
                position.unrealized_pnl = (price - position.avg_price) * total_quantity;
            }
            None => {
                self.positions.insert(
                    symbol.clone(),
                    Position {
                        symbol: symbol.clone(),
                        quantity,
                        avg_price: price,
                        market_value: quantity * price,
                        unrealized_pnl: Decimal::ZERO,
                    },
                );
            }
        }

        self.trades.push(Trade {
            symbol,
            side: TradeSide::Buy,
            quantity,
            price,
            timestamp: Utc::now(),
            realized_pnl: None,
            commission,
        });

        Ok(())
    }

    pub fn execute_sell(
        &mut self,
        symbol: String,
        quantity: Decimal,
        price: Decimal,
    ) -> Result<(), String> {
        let proceeds = quantity * price;
        let commission = proceeds * self.commission_rate;
        self.execute_sell_with_commission(symbol, quantity, price, commission)
    }

    /// Execute a sell with explicit commission (used by exchange-based execution).
    ///
    /// This method allows external systems (like SimulatedExchange) to provide
    /// the exact commission calculated by their fee model, avoiding double-counting.
    pub fn execute_sell_with_commission(
        &mut self,
        symbol: String,
        quantity: Decimal,
        price: Decimal,
        commission: Decimal,
    ) -> Result<(), String> {
        let position = self
            .positions
            .get_mut(&symbol)
            .ok_or("No position to sell")?;

        if quantity > position.quantity {
            return Err(format!(
                "Insufficient position: need {}, available {}",
                quantity, position.quantity
            ));
        }

        let proceeds = quantity * price;
        let net_proceeds = proceeds - commission;

        self.cash += net_proceeds;

        // Calculate realized PnL (includes commission)
        let realized_pnl = (price - position.avg_price) * quantity - commission;

        position.quantity -= quantity;
        if position.quantity == Decimal::ZERO {
            self.positions.remove(&symbol);
        } else {
            position.market_value = position.quantity * price;
            position.unrealized_pnl = (price - position.avg_price) * position.quantity;
        }

        self.trades.push(Trade {
            symbol,
            side: TradeSide::Sell,
            quantity,
            price,
            timestamp: Utc::now(),
            realized_pnl: Some(realized_pnl),
            commission,
        });

        Ok(())
    }

    pub fn total_value(&self) -> Decimal {
        let mut total = self.cash;

        for position in self.positions.values() {
            total += position.market_value;
        }

        total
    }

    pub fn total_realized_pnl(&self) -> Decimal {
        self.trades
            .iter()
            .filter_map(|trade| trade.realized_pnl)
            .sum()
    }

    pub fn total_unrealized_pnl(&self) -> Decimal {
        self.positions.values().map(|pos| pos.unrealized_pnl).sum()
    }

    pub fn total_pnl(&self) -> Decimal {
        self.total_realized_pnl() + self.total_unrealized_pnl()
    }

    pub fn total_commission(&self) -> Decimal {
        self.trades.iter().map(|trade| trade.commission).sum()
    }

    /// Add commission from external source (e.g., exchange fee model).
    ///
    /// Use this when the exchange calculates fees separately from the portfolio's
    /// internal commission rate. The commission is deducted from cash.
    pub fn add_commission(&mut self, amount: Decimal) {
        self.cash -= amount;
    }

    pub fn has_position(&self, symbol: &str) -> bool {
        self.positions.contains_key(symbol)
            && self.positions.get(symbol).unwrap().quantity > Decimal::ZERO
    }

    pub fn get_equity_curve(&self) -> Vec<Decimal> {
        let mut equity_curve = vec![self.initial_capital];
        let mut running_cash = self.initial_capital;
        let mut running_positions: HashMap<String, (Decimal, Decimal)> = HashMap::new(); // (quantity, avg_price)

        for trade in &self.trades {
            match trade.side {
                TradeSide::Buy => {
                    running_cash -= trade.quantity * trade.price + trade.commission;
                    let (curr_qty, curr_avg) = running_positions
                        .get(&trade.symbol)
                        .unwrap_or(&(Decimal::ZERO, Decimal::ZERO));
                    let new_qty = curr_qty + trade.quantity;
                    let new_avg = if new_qty > Decimal::ZERO {
                        (curr_qty * curr_avg + trade.quantity * trade.price) / new_qty
                    } else {
                        Decimal::ZERO
                    };
                    running_positions.insert(trade.symbol.clone(), (new_qty, new_avg));
                }
                TradeSide::Sell => {
                    running_cash += trade.quantity * trade.price - trade.commission;
                    if let Some((curr_qty, _)) = running_positions.get_mut(&trade.symbol) {
                        *curr_qty -= trade.quantity;
                        if *curr_qty <= Decimal::ZERO {
                            running_positions.remove(&trade.symbol);
                        }
                    }
                }
            }

            // Calculate current portfolio value
            let mut portfolio_value = running_cash;
            for (symbol, (quantity, _)) in &running_positions {
                if let Some(current_price) = self.current_prices.get(symbol) {
                    portfolio_value += quantity * current_price;
                }
            }
            equity_curve.push(portfolio_value);
        }

        equity_curve
    }
}
