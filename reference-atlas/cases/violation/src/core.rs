pub fn discount(amount: u64) -> u64 {
    let rate = std::fs::read_to_string("discount.txt").unwrap();
    amount / rate.trim().parse::<u64>().unwrap()
}
