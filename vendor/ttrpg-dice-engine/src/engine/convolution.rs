/// this file has a lot of phrases like "polynomial convolution for PMF computation"
///
/// a PMF is represented as `(min_outcome: i64, Vec<f64>)` where
/// `probs[i]` is the probability of outcome `min_outcome + i`
/// all pub functions return normalized PMFs

pub type Pmf = (i64, Vec<f64>);

const EXPLODE_CAP: u32 = 20;

/// convolve two PMFs
pub fn convolve(a: &Pmf, b: &Pmf) -> Pmf {
    let (a_min, a_probs) = a;
    let (b_min, b_probs) = b;
    let result_min = a_min + b_min;
    let mut result = vec![0.0_f64; a_probs.len() + b_probs.len() - 1];
    for (i, &pa) in a_probs.iter().enumerate() {
        for (j, &pb) in b_probs.iter().enumerate() {
            result[i + j] += pa * pb;
        }
    }
    normalize((result_min, result))
}

/// convolve a PMF with itself n times
pub fn convolve_n(pmf: &Pmf, n: u32) -> Pmf {
    if n == 0 {
        return (0, vec![1.0]);
    }
    let mut result = pmf.clone();
    for _ in 1..n {
        result = convolve(&result, pmf);
    }
    result
}

/// add two PMFs (mixture over the same outcome space. used when combining probability masses)
pub fn add_pmfs(a: &Pmf, b: &Pmf) -> Pmf {
    let (a_min, a_probs) = a;
    let (b_min, b_probs) = b;
    let out_min = (*a_min).min(*b_min);
    let out_max = (a_min + a_probs.len() as i64 - 1).max(b_min + b_probs.len() as i64 - 1);
    let len = (out_max - out_min + 1) as usize;
    let mut result = vec![0.0_f64; len];
    for (i, &p) in a_probs.iter().enumerate() {
        result[(a_min + i as i64 - out_min) as usize] += p;
    }
    for (i, &p) in b_probs.iter().enumerate() {
        result[(b_min + i as i64 - out_min) as usize] += p;
    }
    (out_min, result)
}

/// negate a PMF (multiply all outcomes by -1)
pub fn negate_pmf(pmf: &Pmf) -> Pmf {
    let (min, probs) = pmf;
    let new_min = -(min + probs.len() as i64 - 1);
    let mut new_probs = probs.clone();
    new_probs.reverse();
    (new_min, new_probs)
}

/// add a constant to all outcomes
pub fn shift_pmf(pmf: &Pmf, offset: i64) -> Pmf {
    (pmf.0 + offset, pmf.1.clone())
}

/// PMF of a single standard die (these are digital dice so hopefully uniform over 1..=sides. if not we're in some kind of lovecraft shit I think?)
pub fn single_die_pmf(sides: u32) -> Pmf {
    let p = 1.0 / sides as f64;
    (1, vec![p; sides as usize])
}

/// PMF of a single FATE die. outcomes -1, 0, +1 each w/ probability of 1/3
pub fn fate_die_pmf() -> Pmf {
    (-1, vec![1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0])
}

/// PMF of the sum of n standard dice each with 'sides' faces
pub fn ndx_pmf(n: u32, sides: u32) -> Pmf {
    convolve_n(&single_die_pmf(sides), n)
}

/// PMF of the sum of n FATE dice
pub fn n_fate_pmf(n: u32) -> Pmf {
    convolve_n(&fate_die_pmf(), n)
}

/// PMF of the sum of the highest 'keep' out of 'n' dice each with 'sides' faces
pub fn keep_highest_pmf(n: u32, keep: u32, sides: u32) -> Pmf {
    if keep >= n {
        return ndx_pmf(n, sides);
    }
    order_stats_pmf(n, keep, sides, true)
}

/// PMF of the sum of the lowest 'keep' out of 'n' dice each with 'sides' faces
pub fn keep_lowest_pmf(n: u32, keep: u32, sides: u32) -> Pmf {
    if keep >= n {
        return ndx_pmf(n, sides);
    }
    order_stats_pmf(n, keep, sides, false)
}

/// exact order statistics via multinomial enumeration (big words)
///
/// enumerates all ways to allocate n dice to sface values (a multiset of n values from {1..sides})
/// for each allocation, computes the sum of the top/bottom k dice and the multinomial probability
fn order_stats_pmf(n: u32, keep: u32, sides: u32, keep_high: bool) -> Pmf {
    let n = n as usize;
    let keep = keep as usize;
    let sides = sides as usize;
    let min_out = keep as i64;
    let max_out = (keep * sides) as i64;
    let range = (max_out - min_out + 1) as usize;
    let mut probs = vec![0.0_f64; range];
    let mut counts = vec![0usize; sides];
    enumerate_allocations(
        n,
        sides,
        &mut counts,
        0,
        keep,
        keep_high,
        min_out,
        &mut probs,
    );

    normalize((min_out, probs))
}

///recursively compute exact PMF for rolls w/ mods like keep highest or drop lowest
fn enumerate_allocations(
    remaining: usize,
    sides: usize,
    counts: &mut Vec<usize>,
    face_idx: usize,
    keep: usize,
    keep_high: bool,
    min_out: i64,
    probs: &mut Vec<f64>,
) {
    if face_idx == sides {
        if remaining != 0 {
            return;
        }
        let n: usize = counts.iter().sum();
        let sum = compute_topk_sum(counts, keep, keep_high);
        let p = multinomial_prob(counts, n, sides);
        let idx = (sum - min_out) as usize;
        if idx < probs.len() {
            probs[idx] += p;
        }
        return;
    }
    for c in 0..=remaining {
        counts[face_idx] = c;
        enumerate_allocations(
            remaining - c,
            sides,
            counts,
            face_idx + 1,
            keep,
            keep_high,
            min_out,
            probs,
        );
    }
    counts[face_idx] = 0;
}

/// sum of the top keep (or bottom keep if !keep_high) dice values from the allocation
fn compute_topk_sum(counts: &[usize], keep: usize, keep_high: bool) -> i64 {
    // counts[f] = how many dice show face (f+1)
    let mut sum = 0i64;
    let mut remaining = keep;
    if keep_high {
        // shimmy around from highest face down
        for (f, &c) in counts.iter().enumerate().rev() {
            if remaining == 0 {
                break;
            }
            let take = c.min(remaining);
            sum += (f + 1) as i64 * take as i64;
            remaining -= take;
        }
    } else {
        // giddy and hup from lowest face up
        for (f, &c) in counts.iter().enumerate() {
            if remaining == 0 {
                break;
            }
            let take = c.min(remaining);
            sum += (f + 1) as i64 * take as i64;
            remaining -= take;
        }
    }
    sum
}

/// calculates the probability of rolling a specific distribution of face values when you throw n dice
fn multinomial_prob(counts: &[usize], n: usize, sides: usize) -> f64 {
    let mut coeff = 1.0_f64;
    let mut n_rem = n;
    for &c in counts {
        if c == 0 {
            continue;
        }
        for i in 0..c {
            coeff *= (n_rem - i) as f64 / (i + 1) as f64;
        }
        n_rem -= c;
    }
    coeff * (1.0 / sides as f64).powi(n as i32)
}

/// PMF of an exploding die (faces >= threshold cause the die to be rerolled and added).
/// capped at `EXPLODE_CAP` extra layers (you can only explode so much)
pub fn exploding_pmf(sides: u32, threshold: u32) -> Pmf {
    if sides == 0 {
        return (0, vec![1.0]);
    }
    if threshold > sides {
        return single_die_pmf(sides);
    }
    let p = 1.0 / sides as f64;
    let n_explode_faces = (sides - threshold + 1) as usize;
    let p_explode = n_explode_faces as f64 * p;

    let mut result: Pmf = {
        let mut v = vec![0.0_f64; (threshold as usize).saturating_sub(1)];
        for x in v.iter_mut() {
            *x = p;
        }
        (1, v)
    };

    let explode_face_pmf: Pmf = (threshold as i64, vec![p; n_explode_faces]);
    let single = single_die_pmf(sides);

    let mut chain = explode_face_pmf;
    for _ in 0..EXPLODE_CAP {
        let contribution = convolve(&chain, &single);
        result = add_pmfs(&result, &contribution);

        let next_chain: Vec<f64> = chain.1.iter().map(|&cp| cp * p_explode).collect();
        chain = (chain.0, next_chain);
        if chain.1.iter().all(|&x| x < 1e-15) {
            break;
        }
    }

    normalize(result)
}

/// PMF of the number of successes
pub fn success_count_pmf(n: u32, p_success: f64) -> Pmf {
    let p = p_success;
    let q = 1.0 - p;
    let mut probs = Vec::with_capacity(n as usize + 1);
    let mut binom = 1.0_f64;
    for k in 0..=(n as usize) {
        probs.push(binom * p.powi(k as i32) * q.powi((n as usize - k) as i32));
        if k < n as usize {
            binom *= (n as usize - k) as f64 / (k + 1) as f64;
        }
    }
    normalize((0, probs))
}

///normalize a PMF so all probabilities sum to 1.0
pub fn normalize(pmf: Pmf) -> Pmf {
    let (min, probs) = pmf;
    let sum: f64 = probs.iter().sum();
    if sum == 0.0 || (sum - 1.0).abs() < 1e-12 {
        return (min, probs);
    }
    (min, probs.into_iter().map(|p| p / sum).collect())
}

/// compute the mean of a PMF
pub fn pmf_mean(pmf: &Pmf) -> f64 {
    let (min, probs) = pmf;
    probs
        .iter()
        .enumerate()
        .map(|(i, &p)| (min + i as i64) as f64 * p)
        .sum()
}

/// compute the variance of a PMF given its mean
pub fn pmf_variance(pmf: &Pmf, mean: f64) -> f64 {
    let (min, probs) = pmf;
    probs
        .iter()
        .enumerate()
        .map(|(i, &p)| p * ((min + i as i64) as f64 - mean).powi(2))
        .sum()
}

/// find the outcome at the given percentile (0.0–1.0 inclusive)
pub fn pmf_percentile(pmf: &Pmf, pct: f64) -> i64 {
    let (min, probs) = pmf;
    let mut cumulative = 0.0;
    for (i, &p) in probs.iter().enumerate() {
        cumulative += p;
        if cumulative >= pct {
            return min + i as i64;
        }
    }
    min + probs.len() as i64 - 1
}

/// return (outcome_probability, cumulative_probability) for a given outcome
pub fn pmf_position(pmf: &Pmf, value: i64) -> (f64, f64) {
    let (min, probs) = pmf;
    let idx = value - min;
    if idx < 0 {
        return (0.0, 0.0);
    }
    if idx as usize >= probs.len() {
        return (0.0, 1.0);
    }
    let outcome_prob = probs[idx as usize];
    let cumulative: f64 = probs[..=(idx as usize)].iter().sum();
    (outcome_prob, cumulative)
}
