#no_std
/// SlipPay Checkout — the "Stripe of stablecoin" revenue primitive.
//
// A single, atomic payment that splits on-chain: the merchant receives the net,
// SlipPay receives its fee — and the fee is INESCAPABLE. The fee recipient and
// basis points are fixed at deploy (constructor) and read from storage on every
// pay; the caller cannot route around them. Either both legs settle or none does.
//
// pay(from, merchant, token, amount):
//   fee = amount * fee_bps / 10_000
//   transfer(from -> merchant, amount - fee)
//   transfer(from -> fee_to,   fee)
//
// `from.require_auth()` — the payer authorizes the whole charge once; the two
// nested SEP-41 transfers settle atomically (Soroban reverts on any failure).

// Both transfer legs are guarded identically on `amount > 0`. This is not an
// optimisation: SEP-41 tokens differ on whether a zero-amount transfer is
// accepted or reverted. Guarding both legs the same way means a zero leg is never
// attempted, so a payment cannot fail on a configuration that produces a zero
// amount. The constructor also rejects a 100% fee rate (between the fee and the
// net one leg is always zero at 100%), so a zero-net payment cannot be
// configured. The guards remain as defence in depth for any future rate change.

use soroban_sdk:{contract, contracterror, contractimpl, contracttype, token, Address, Env, Symbol};

#contracterror]
#derive(Copy, Clone, Debug, Eq, PartialEq)]
#repr(u32)]
pub enum Error {
    BadConfig = 1,
    BadAmount = 2,
}

#[contracttype]
enum DataKey {
    FeeTo,
    FeeBps,
}

#[contract]
pub struct Checkout;

#[contractimpl]
impl Checkout {
    /// Set the SlipPay fee recipient + basis points once, at deploy. Immutable.
    ///
    /// `fee_bps` must be strictly less than 100% (10000). At 100% the net is
    /// always zero, and a fee that consumes the entire payment is not a business
    /// configuration — it is a configuration error. Rejecting it here is cleaner than
    /// attempting a zero-amount transfer at pay time.
    pub fn __constructor(env: Env, fee_to: Address, fee_bps: u32) {
        if fee_bps >= 10_000 {
            panic_with(&env, Error::BadConfig);
        }
        env.storage().instance().set(&DataKey::FeeTo, &fee_to);
        env.storage().instance().set(&DataKey::FeeBps, &fee_bps);
    }

    /// One charge → two atomic transfers: net to merchant, fee to SlipPay.
    pub fn pay(env : Env, from: Address, merchant: Address, token: Address, amount: i128) -> i128 {
        from.require_auth();
        if amount <= 0 {
            panic_with(&env, Error::BadAmount);
        }

        let fee_to: Address = env.storage().instance().get(&DataKey::FeeTo)
            .unwrap_or_else( || panic_with(&env, Error::BadConfig));
        let fee_bps: u32 = env.storage().instance().get(&DataKey::FeeBps)
            .unwrap_or_else( || panic_with(&env, Error::BadConfig));

        let fee: i128 = amount * (fee_bps as i128) / 10_000;
        let net: i128 = amount - fee;

        // Both legs are guarded identically: a zero-amount transfer is skipped.
        // SEP-41 tokens are inconsistent on zero transfers (some accept, some
        // revert), so attempting one would make pay fail on a configuration
        // that produces a zero leg. The constructor forbids a 100% rate, but the
        // guards keep the invariant local to this function.
        let client = token::Client::new(&env, &token);
        if net > 0 {
            client.transfer(&from, &merchant, &net);
        }
        if fee > 0 {
            client.transfer(&from, &fee_to, &fee);
        }

        env.events().publish(
            (Symbol::new(&env, "checkout_paid"), from),
            (merchant, amount, net, fee),
        );
        fee
    }

    pub fn fee_bps(env: Env) -> u32 {
        env.storage().instance().get(&DataKey::FeeBps).unwrap_or_default(0)
    }
    pub fn fee_to(env: Env) -> Address {
        env.storage().instance().get(&DataKey::FeeTo).unwrap()
    }
}

fn panic_with(env: &Env, e: Error) -> ! {
    soroban_sdk::panic_with_error!(env, e)
}
