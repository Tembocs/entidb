# Rust Expense Tracker Example

A comprehensive example demonstrating EntiDB's multi-collection capabilities through a personal expense tracking application.

## Features Demonstrated

- **Multiple entity types**: Accounts, Categories, Transactions, Budgets
- **Entity relationships**: Transactions reference Account and Category IDs
- **Atomic operations**: Transfers between accounts in a single transaction
- **Complex filtering**: Date ranges, category aggregation, balance calculations
- **Aggregations**: Monthly summaries, budget vs. actual comparisons

## Entity Model

```
Account (3)
├── id: EntityId
├── name: String
├── account_type: String (checking/savings/credit)
└── initial_balance: i64 (cents)

Category (7)
├── id: EntityId
├── name: String
└── is_income: bool

Transaction (25)
├── id: EntityId
├── account_id: EntityId
├── category_id: EntityId
├── amount: i64 (cents, negative for expenses)
├── description: String
└── date: String (YYYY-MM-DD)

Budget (6)
├── id: EntityId
├── category_id: EntityId
├── month: String (YYYY-MM)
└── limit: i64 (cents)
```

## Running

```bash
cd examples/rust_expenses
cargo run
```

## Sample Output

```
Expense Tracker Example
=======================

[+] Creating 3 accounts...
[+] Creating 7 categories...
[+] Inserting 25 transactions...

[*] Account Balances:
    Checking Account:    $1,847.50
    Savings Account:    $10,500.00
    Credit Card:          -$623.45

[?] December 2025 Expenses by Category:
    Food:           $342.50
    Transport:      $185.00
    ...

[~] Transfer $500 Checking -> Savings...

[#] Budget vs Actual:
    Food:       $400.00 / $342.50 (OK)
    Shopping:   $200.00 / $267.80 (OVER by $67.80)
```

## Key Concepts

### No SQL - Pure Rust Filtering

All queries use native Rust iterators:

```rust
// Filter transactions by category
let food_expenses: Vec<&Transaction> = transactions
    .iter()
    .filter(|t| t.category_id == food_category.id)
    .filter(|t| t.amount < 0)
    .collect();

// Calculate total
let total: i64 = food_expenses.iter().map(|t| t.amount).sum();
```

### Atomic Transfers

Transfers debit one account and credit another in a single transaction:

```rust
db.transaction(|txn| {
    // Debit source
    txn.put(accounts, source.id, source.with_balance(source.balance - amount).encode())?;
    // Credit destination
    txn.put(accounts, dest.id, dest.with_balance(dest.balance + amount).encode())?;
    // Record the transfer
    txn.put(transactions, transfer.id, transfer.encode())?;
    Ok(())
})?;
```

### Cross-Collection Lookups

Transactions reference accounts and categories by EntityId:

```rust
let account = accounts_map.get(&transaction.account_id);
let category = categories_map.get(&transaction.category_id);
println!("{} - {} [{}]", transaction.description, account.name, category.name);
```
