use std::{
	hint::black_box,
	ops::RangeInclusive,
	thread::sleep,
	time::{Duration, Instant},
};

use geode::{Archetype, Storage, NO_LABEL};

fn main() {
	// Bench 1
	{
		let foo = vec![3];

		bench(500, 500_000..=1_000_000, || foo[0]);
	}

	// Bench 2
	{
		let mut arch = Archetype::<()>::new(NO_LABEL);
		let entity = arch.spawn(NO_LABEL);

		let mut target = Storage::new();
		target.add(entity, 3);

		bench(500, 500_000..=1_000_000, || target[entity]);
	}

	// Bench 3
	{
		let mut arch = Archetype::<()>::new(NO_LABEL);
		let entity = arch.spawn(NO_LABEL);

		let mut target = Storage::new();
		target.add(entity, 3);

		for _ in 0..1000 {
			let mut arch = Archetype::<()>::new(NO_LABEL);
			let entity = arch.spawn(NO_LABEL);
			target.add(entity, 4);
			std::mem::forget(arch);
		}

		bench(500, 500_000..=1_000_000, || target[entity]);
	}
}

fn bench<F, R>(max_iter: u32, count_range: RangeInclusive<u32>, mut f: F)
where
	F: FnMut() -> R,
{
	let mut tpi_accum = 0f64;
	let mut tpi_samples = 0f64;

	for _ in 0..max_iter {
		let count = fastrand::u32(count_range.clone());
		let start = Instant::now();

		for _ in 0..count {
			black_box(f());
		}

		let elapsed = start.elapsed();
		let tpi = elapsed.as_nanos() as f64 / count as f64;
		tpi_accum += tpi;
		tpi_samples += 1.;

		let average_tpi = tpi_accum / tpi_samples;
		println!(
			"{tpi_samples}: Ran {count} iterations in {elapsed:?}.\n    \
			 TPI: {tpi:.4}ns. Average TPI: {average_tpi:.4}ns."
		);

		sleep(Duration::ZERO);
	}

	let average_tpi = tpi_accum / tpi_samples;
	println!("Finished {max_iter} iterations. Average TPI: {average_tpi:.4}ns.");

	sleep(Duration::ZERO);
}
