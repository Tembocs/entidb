//! Personal Expense Tracker Example
//!
//! Demonstrates EntiDB's multi-collection capabilities:
//! - Multiple entity types with relationships
//! - Atomic transfers between accounts
//! - Complex filtering and aggregation with Rust iterators
//! - Budget tracking and comparisons
//!
//! Run with: cargo run -p rust_expenses

use entidb_codec::{from_cbor, to_canonical_cbor, Value};
use entidb_core::{Database, EntityId};
use std::collections::HashMap;

// ============================================================================
// Entity: Account
// ============================================================================

#[derive(Debug, Clone)]
struct Account {
    id: EntityId,
    name: String,
    account_type: String, // "checking", "savings", "credit"
    balance: i64,         // in cents
}

impl Account {
    fn new(name: &str, account_type: &str, initial_balance: i64) -> Self {
        Self {
            id: EntityId::new(),
            name: name.to_string(),
            account_type: account_type.to_string(),
            balance: initial_balance,
        }
    }

    fn with_balance(&self, new_balance: i64) -> Self {
        Self {
            balance: new_balance,
            ..self.clone()
        }
    }

    fn encode(&self) -> Vec<u8> {
        let pairs = vec![
            (Value::Text("account_type".into()), Value::Text(self.account_type.clone())),
            (Value::Text("balance".into()), Value::Integer(self.balance)),
            (Value::Text("name".into()), Value::Text(self.name.clone())),
        ];
        to_canonical_cbor(&Value::Map(pairs)).expect("encoding should succeed")
    }

    #[allow(dead_code)]
    fn decode(id: EntityId, bytes: &[u8]) -> Result<Self, String> {
        let value = from_cbor(bytes).map_err(|e| e.to_string())?;
        if let Value::Map(pairs) = value {
            let mut name = None;
            let mut account_type = None;
            let mut balance = None;

            for (k, v) in pairs {
                if let Value::Text(key) = k {
                    match key.as_str() {
                        "name" => {
                            if let Value::Text(t) = v {
                                name = Some(t);
                            }
                        }
                        "account_type" => {
                            if let Value::Text(t) = v {
                                account_type = Some(t);
                            }
                        }
                        "balance" => {
                            if let Value::Integer(i) = v {
                                balance = Some(i);
                            }
                        }
                        _ => {}
                    }
                }
            }

            Ok(Account {
                id,
                name: name.ok_or("missing name")?,
                account_type: account_type.ok_or("missing account_type")?,
                balance: balance.ok_or("missing balance")?,
            })
        } else {
            Err("expected map".into())
        }
    }

    #[allow(dead_code)]
    fn format_balance(&self) -> String {
        format_cents(self.balance)
    }
}

// ============================================================================
// Entity: Category
// ============================================================================

#[derive(Debug, Clone)]
struct Category {
    id: EntityId,
    name: String,
    is_income: bool,
}

impl Category {
    fn new(name: &str, is_income: bool) -> Self {
        Self {
            id: EntityId::new(),
            name: name.to_string(),
            is_income,
        }
    }

    fn encode(&self) -> Vec<u8> {
        let pairs = vec![
            (Value::Text("is_income".into()), Value::Bool(self.is_income)),
            (Value::Text("name".into()), Value::Text(self.name.clone())),
        ];
        to_canonical_cbor(&Value::Map(pairs)).expect("encoding should succeed")
    }

    #[allow(dead_code)]
    fn decode(id: EntityId, bytes: &[u8]) -> Result<Self, String> {
        let value = from_cbor(bytes).map_err(|e| e.to_string())?;
        if let Value::Map(pairs) = value {
            let mut name = None;
            let mut is_income = None;

            for (k, v) in pairs {
                if let Value::Text(key) = k {
                    match key.as_str() {
                        "name" => {
                            if let Value::Text(t) = v {
                                name = Some(t);
                            }
                        }
                        "is_income" => {
                            if let Value::Bool(b) = v {
                                is_income = Some(b);
                            }
                        }
                        _ => {}
                    }
                }
            }

            Ok(Category {
                id,
                name: name.ok_or("missing name")?,
                is_income: is_income.ok_or("missing is_income")?,
            })
        } else {
            Err("expected map".into())
        }
    }
}

// ============================================================================
// Entity: Transaction
// ============================================================================

#[derive(Debug, Clone)]
struct Transaction {
    id: EntityId,
    account_id: EntityId,
    category_id: EntityId,
    amount: i64, // cents, negative for expenses
    description: String,
    date: String, // YYYY-MM-DD
}

impl Transaction {
    fn new(
        account_id: EntityId,
        category_id: EntityId,
        amount: i64,
        description: &str,
        date: &str,
    ) -> Self {
        Self {
            id: EntityId::new(),
            account_id,
            category_id,
            amount,
            description: description.to_string(),
            date: date.to_string(),
        }
    }

    fn encode(&self) -> Vec<u8> {
        let pairs = vec![
            (Value::Text("account_id".into()), Value::Bytes(self.account_id.as_bytes().to_vec())),
            (Value::Text("amount".into()), Value::Integer(self.amount)),
            (Value::Text("category_id".into()), Value::Bytes(self.category_id.as_bytes().to_vec())),
            (Value::Text("date".into()), Value::Text(self.date.clone())),
            (Value::Text("description".into()), Value::Text(self.description.clone())),
        ];
        to_canonical_cbor(&Value::Map(pairs)).expect("encoding should succeed")
    }

    #[allow(dead_code)]
    fn decode(id: EntityId, bytes: &[u8]) -> Result<Self, String> {
        let value = from_cbor(bytes).map_err(|e| e.to_string())?;
        if let Value::Map(pairs) = value {
            let mut account_id = None;
            let mut category_id = None;
            let mut amount = None;
            let mut description = None;
            let mut date = None;

            for (k, v) in pairs {
                if let Value::Text(key) = k {
                    match key.as_str() {
                        "account_id" => {
                            if let Value::Bytes(b) = v {
                                if b.len() == 16 {
                                    let arr: [u8; 16] = b.try_into().map_err(|_| "invalid entity id")?;
                                    account_id = Some(EntityId::from_bytes(arr));
                                }
                            }
                        }
                        "category_id" => {
                            if let Value::Bytes(b) = v {
                                if b.len() == 16 {
                                    let arr: [u8; 16] = b.try_into().map_err(|_| "invalid entity id")?;
                                    category_id = Some(EntityId::from_bytes(arr));
                                }
                            }
                        }
                        "amount" => {
                            if let Value::Integer(i) = v {
                                amount = Some(i);
                            }
                        }
                        "description" => {
                            if let Value::Text(t) = v {
                                description = Some(t);
                            }
                        }
                        "date" => {
                            if let Value::Text(t) = v {
                                date = Some(t);
                            }
                        }
                        _ => {}
                    }
                }
            }

            Ok(Transaction {
                id,
                account_id: account_id.ok_or("missing account_id")?,
                category_id: category_id.ok_or("missing category_id")?,
                amount: amount.ok_or("missing amount")?,
                description: description.ok_or("missing description")?,
                date: date.ok_or("missing date")?,
            })
        } else {
            Err("expected map".into())
        }
    }

    fn format_amount(&self) -> String {
        format_cents(self.amount)
    }
}

// ============================================================================
// Entity: Budget
// ============================================================================

#[derive(Debug, Clone)]
struct Budget {
    id: EntityId,
    category_id: EntityId,
    month: String, // YYYY-MM
    limit: i64,    // cents
}

impl Budget {
    fn new(category_id: EntityId, month: &str, limit: i64) -> Self {
        Self {
            id: EntityId::new(),
            category_id,
            month: month.to_string(),
            limit,
        }
    }

    fn encode(&self) -> Vec<u8> {
        let pairs = vec![
            (Value::Text("category_id".into()), Value::Bytes(self.category_id.as_bytes().to_vec())),
            (Value::Text("limit".into()), Value::Integer(self.limit)),
            (Value::Text("month".into()), Value::Text(self.month.clone())),
        ];
        to_canonical_cbor(&Value::Map(pairs)).expect("encoding should succeed")
    }

    #[allow(dead_code)]
    fn decode(id: EntityId, bytes: &[u8]) -> Result<Self, String> {
        let value = from_cbor(bytes).map_err(|e| e.to_string())?;
        if let Value::Map(pairs) = value {
            let mut category_id = None;
            let mut month = None;
            let mut limit = None;

            for (k, v) in pairs {
                if let Value::Text(key) = k {
                    match key.as_str() {
                        "category_id" => {
                            if let Value::Bytes(b) = v {
                                if b.len() == 16 {
                                    let arr: [u8; 16] = b.try_into().map_err(|_| "invalid entity id")?;
                                    category_id = Some(EntityId::from_bytes(arr));
                                }
                            }
                        }
                        "month" => {
                            if let Value::Text(t) = v {
                                month = Some(t);
                            }
                        }
                        "limit" => {
                            if let Value::Integer(i) = v {
                                limit = Some(i);
                            }
                        }
                        _ => {}
                    }
                }
            }

            Ok(Budget {
                id,
                category_id: category_id.ok_or("missing category_id")?,
                month: month.ok_or("missing month")?,
                limit: limit.ok_or("missing limit")?,
            })
        } else {
            Err("expected map".into())
        }
    }

    fn format_limit(&self) -> String {
        format_cents(self.limit)
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn format_cents(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.abs();
    format!("{}${}.{:02}", sign, abs / 100, abs % 100)
}

// ============================================================================
// Main
// ============================================================================

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Expense Tracker Example");
    println!("=======================\n");

    let db = Database::open_in_memory()?;

    // Get collections
    let accounts_coll = db.collection("accounts");
    let categories_coll = db.collection("categories");
    let transactions_coll = db.collection("transactions");
    let budgets_coll = db.collection("budgets");

    // ========================================================================
    // Create Accounts
    // ========================================================================
    println!("[+] Creating 3 accounts...");

    let checking = Account::new("Checking Account", "checking", 250000); // $2,500.00
    let savings = Account::new("Savings Account", "savings", 1000000);   // $10,000.00
    let credit = Account::new("Credit Card", "credit", -45000);          // -$450.00

    let accounts = vec![checking.clone(), savings.clone(), credit.clone()];

    db.transaction(|txn| {
        for account in &accounts {
            txn.put(accounts_coll, account.id, account.encode())?;
        }
        Ok(())
    })?;

    // ========================================================================
    // Create Categories
    // ========================================================================
    println!("[+] Creating 7 categories...");

    let cat_income = Category::new("Income", true);
    let cat_food = Category::new("Food", false);
    let cat_transport = Category::new("Transport", false);
    let cat_utilities = Category::new("Utilities", false);
    let cat_entertainment = Category::new("Entertainment", false);
    let cat_shopping = Category::new("Shopping", false);
    let cat_health = Category::new("Health", false);

    let categories = vec![
        cat_income.clone(),
        cat_food.clone(),
        cat_transport.clone(),
        cat_utilities.clone(),
        cat_entertainment.clone(),
        cat_shopping.clone(),
        cat_health.clone(),
    ];

    db.transaction(|txn| {
        for category in &categories {
            txn.put(categories_coll, category.id, category.encode())?;
        }
        Ok(())
    })?;

    // Build lookup maps
    let category_map: HashMap<EntityId, &Category> = categories.iter().map(|c| (c.id, c)).collect();

    // ========================================================================
    // Create Transactions (25 entries across 2 weeks)
    // ========================================================================
    println!("[+] Inserting 25 transactions...");

    let transactions = vec![
        // Income
        Transaction::new(checking.id, cat_income.id, 350000, "Salary deposit", "2025-12-01"),
        Transaction::new(checking.id, cat_income.id, 15000, "Freelance payment", "2025-12-10"),
        
        // Food (7 transactions)
        Transaction::new(checking.id, cat_food.id, -8550, "Grocery Store", "2025-12-02"),
        Transaction::new(credit.id, cat_food.id, -4275, "Restaurant dinner", "2025-12-05"),
        Transaction::new(checking.id, cat_food.id, -3200, "Coffee shop", "2025-12-07"),
        Transaction::new(credit.id, cat_food.id, -6500, "Grocery Store", "2025-12-09"),
        Transaction::new(checking.id, cat_food.id, -2800, "Fast food", "2025-12-12"),
        Transaction::new(credit.id, cat_food.id, -5100, "Restaurant lunch", "2025-12-14"),
        Transaction::new(checking.id, cat_food.id, -3825, "Bakery", "2025-12-15"),
        
        // Transport (4 transactions)
        Transaction::new(checking.id, cat_transport.id, -5000, "Gas station", "2025-12-03"),
        Transaction::new(credit.id, cat_transport.id, -3500, "Uber ride", "2025-12-06"),
        Transaction::new(checking.id, cat_transport.id, -5500, "Gas station", "2025-12-11"),
        Transaction::new(checking.id, cat_transport.id, -4500, "Parking fee", "2025-12-13"),
        
        // Utilities (2 transactions)
        Transaction::new(checking.id, cat_utilities.id, -12000, "Electric bill", "2025-12-01"),
        Transaction::new(checking.id, cat_utilities.id, -8500, "Internet bill", "2025-12-05"),
        
        // Entertainment (3 transactions)
        Transaction::new(credit.id, cat_entertainment.id, -1599, "Streaming subscription", "2025-12-01"),
        Transaction::new(credit.id, cat_entertainment.id, -4500, "Movie tickets", "2025-12-08"),
        Transaction::new(checking.id, cat_entertainment.id, -6000, "Concert tickets", "2025-12-14"),
        
        // Shopping (4 transactions)
        Transaction::new(credit.id, cat_shopping.id, -8999, "Electronics store", "2025-12-04"),
        Transaction::new(credit.id, cat_shopping.id, -4500, "Clothing store", "2025-12-07"),
        Transaction::new(checking.id, cat_shopping.id, -6780, "Home supplies", "2025-12-10"),
        Transaction::new(credit.id, cat_shopping.id, -6500, "Gift shopping", "2025-12-13"),
        
        // Health (2 transactions)
        Transaction::new(checking.id, cat_health.id, -3500, "Pharmacy", "2025-12-02"),
        Transaction::new(checking.id, cat_health.id, -2500, "Gym membership", "2025-12-01"),
    ];

    db.transaction(|txn| {
        for trans in &transactions {
            txn.put(transactions_coll, trans.id, trans.encode())?;
        }
        Ok(())
    })?;

    // ========================================================================
    // Create Budgets for December 2025
    // ========================================================================
    println!("[+] Creating 6 budgets for December 2025...\n");

    let budgets = vec![
        Budget::new(cat_food.id, "2025-12", 40000),          // $400
        Budget::new(cat_transport.id, "2025-12", 20000),     // $200
        Budget::new(cat_utilities.id, "2025-12", 25000),     // $250
        Budget::new(cat_entertainment.id, "2025-12", 15000), // $150
        Budget::new(cat_shopping.id, "2025-12", 20000),      // $200
        Budget::new(cat_health.id, "2025-12", 10000),        // $100
    ];

    db.transaction(|txn| {
        for budget in &budgets {
            txn.put(budgets_coll, budget.id, budget.encode())?;
        }
        Ok(())
    })?;

    // ========================================================================
    // Display Account Balances (calculated from initial + transactions)
    // ========================================================================
    println!("[*] Account Balances:");

    let mut account_balances: HashMap<EntityId, i64> = accounts.iter().map(|a| (a.id, a.balance)).collect();

    for trans in &transactions {
        if let Some(balance) = account_balances.get_mut(&trans.account_id) {
            *balance += trans.amount;
        }
    }

    for account in &accounts {
        let balance = account_balances[&account.id];
        println!("    {:20} {:>12}", account.name, format_cents(balance));
    }

    // ========================================================================
    // Expenses by Category (December 2025)
    // ========================================================================
    println!("\n[?] December 2025 Expenses by Category:");

    let mut category_totals: HashMap<EntityId, i64> = HashMap::new();

    for trans in &transactions {
        if trans.amount < 0 {
            *category_totals.entry(trans.category_id).or_insert(0) += trans.amount.abs();
        }
    }

    for cat in &categories {
        if !cat.is_income {
            let total = category_totals.get(&cat.id).copied().unwrap_or(0);
            println!("    {:20} {:>12}", cat.name, format_cents(total));
        }
    }

    // ========================================================================
    // Recent Transactions (last 5)
    // ========================================================================
    println!("\n[*] Recent Transactions:");

    let mut sorted_transactions = transactions.clone();
    sorted_transactions.sort_by(|a, b| b.date.cmp(&a.date));

    for trans in sorted_transactions.iter().take(5) {
        let cat_name = category_map
            .get(&trans.category_id)
            .map(|c| c.name.as_str())
            .unwrap_or("Unknown");
        println!(
            "    {} {:25} {:>10} [{}]",
            trans.date,
            trans.description,
            trans.format_amount(),
            cat_name
        );
    }

    // ========================================================================
    // Atomic Transfer: Checking -> Savings
    // ========================================================================
    println!("\n[~] Transfer $500.00 from Checking to Savings...");

    let transfer_amount: i64 = 50000; // $500.00

    // Create a new transfer transaction (recorded as expense from checking perspective)
    let transfer_out = Transaction::new(
        checking.id,
        cat_income.id, // Using income category for transfers (neutral)
        -transfer_amount,
        "Transfer to Savings",
        "2025-12-15",
    );
    let transfer_in = Transaction::new(
        savings.id,
        cat_income.id,
        transfer_amount,
        "Transfer from Checking",
        "2025-12-15",
    );

    // Update account balances atomically
    let new_checking_balance = account_balances[&checking.id] - transfer_amount;
    let new_savings_balance = account_balances[&savings.id] + transfer_amount;

    let updated_checking = checking.with_balance(new_checking_balance);
    let updated_savings = savings.with_balance(new_savings_balance);

    db.transaction(|txn| {
        // Update both accounts
        txn.put(accounts_coll, updated_checking.id, updated_checking.encode())?;
        txn.put(accounts_coll, updated_savings.id, updated_savings.encode())?;
        // Record both sides of the transfer
        txn.put(transactions_coll, transfer_out.id, transfer_out.encode())?;
        txn.put(transactions_coll, transfer_in.id, transfer_in.encode())?;
        Ok(())
    })?;

    println!("    Checking: {} -> {}", 
        format_cents(account_balances[&checking.id]),
        format_cents(new_checking_balance));
    println!("    Savings:  {} -> {}", 
        format_cents(account_balances[&savings.id]),
        format_cents(new_savings_balance));

    // Update our local tracking
    account_balances.insert(checking.id, new_checking_balance);
    account_balances.insert(savings.id, new_savings_balance);

    // ========================================================================
    // Budget vs Actual
    // ========================================================================
    println!("\n[#] Budget vs Actual (December 2025):");

    for budget in &budgets {
        let cat = category_map.get(&budget.category_id);
        let cat_name = cat.map(|c| c.name.as_str()).unwrap_or("Unknown");
        let spent = category_totals.get(&budget.category_id).copied().unwrap_or(0);
        let diff = budget.limit - spent;
        
        let status = if diff >= 0 {
            format!("OK, {} left", format_cents(diff))
        } else {
            format!("OVER by {}", format_cents(-diff))
        };

        println!(
            "    {:20} {:>10} / {:>10} ({})",
            cat_name,
            format_cents(spent),
            budget.format_limit(),
            status
        );
    }

    // ========================================================================
    // Summary Statistics
    // ========================================================================
    println!("\n[#] Summary:");

    let total_income: i64 = transactions.iter().filter(|t| t.amount > 0).map(|t| t.amount).sum();
    let total_expenses: i64 = transactions.iter().filter(|t| t.amount < 0).map(|t| t.amount.abs()).sum();
    let net = total_income - total_expenses;

    println!("    Total Income:      {:>12}", format_cents(total_income));
    println!("    Total Expenses:    {:>12}", format_cents(total_expenses));
    println!("    Net:               {:>12}", format_cents(net));
    println!("    Transactions:      {:>12}", transactions.len());

    // Verify data in database
    let db_accounts = db.list(accounts_coll)?;
    let db_transactions = db.list(transactions_coll)?;
    let db_budgets = db.list(budgets_coll)?;

    println!("\n[*] Database Contents:");
    println!("    Accounts:          {:>12}", db_accounts.len());
    println!("    Categories:        {:>12}", categories.len());
    println!("    Transactions:      {:>12}", db_transactions.len()); // 25 + 2 transfer
    println!("    Budgets:           {:>12}", db_budgets.len());

    db.close()?;
    println!("\n[*] Database closed");

    Ok(())
}
