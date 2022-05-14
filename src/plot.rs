use plotters::data::fitting_range;
use plotters::prelude::*;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, prelude::*, BufReader};

use crate::Data;

fn read_data<BR: BufRead>(reader: BR) -> HashMap<(String, String), Vec<f64>> {
    let mut ds = HashMap::new();
    for l in reader.lines() {
        let line = l.unwrap();
        let tuple: Vec<&str> = line.split('\t').collect();
        if tuple.len() == 3 {
            let key = (String::from(tuple[0]), String::from(tuple[1]));
            let entry = ds.entry(key).or_insert_with(Vec::new);
            entry.push(tuple[2].parse::<f64>().unwrap());
        }
    }
    ds
}

const OUT_FILE_NAME: &'static str = "plotters-doc-data/boxplot.svg";
static LABELS: [&'static str; 7] = [
    "first kernel print",
    "start init process",
    "unser input",
    "kexec load time",
    "kexec: first kernel print",
    "kexec: start init process",
    "kexec: unser input",
];
pub fn plot(data: Vec<Data>) -> Result<(), Box<dyn std::error::Error>> {
    let root = SVGBackend::new(OUT_FILE_NAME, (1024, 768)).into_drawing_area();
    root.fill(&WHITE)?;

    let root = root.margin(5u32, 5u32, 5u32, 5u32);

    let quartiles = [
        Quartiles::new(
            &data
                .iter()
                .map(|d| d.normal_start.entry.as_secs_f64())
                .collect::<Vec<_>>(),
        ),
        Quartiles::new(
            &data
                .iter()
                .map(|d| d.normal_start.run_init.as_secs_f64())
                .collect::<Vec<_>>(),
        ),
        Quartiles::new(
            &data
                .iter()
                .map(|d| d.normal_start.login.as_secs_f64())
                .collect::<Vec<_>>(),
        ),
        Quartiles::new(
            &data
                .iter()
                .map(|d| d.kexec_load_time.as_secs_f64())
                .collect::<Vec<_>>(),
        ),
        Quartiles::new(
            &data
                .iter()
                .map(|d| d.kexec_start.entry.as_secs_f64())
                .collect::<Vec<_>>(),
        ),
        Quartiles::new(
            &data
                .iter()
                .map(|d| d.kexec_start.run_init.as_secs_f64())
                .collect::<Vec<_>>(),
        ),
        Quartiles::new(
            &data
                .iter()
                .map(|d| d.kexec_start.login.as_secs_f64())
                .collect::<Vec<_>>(),
        ),
    ];

    let all_values = quartiles
        .iter()
        .map(|q| q.values())
        .flatten()
        .collect::<Vec<_>>();
    let values_range = fitting_range(all_values.iter());
    let mut chart = ChartBuilder::on(&root)
        .x_label_area_size(40u32)
        .y_label_area_size(40u32)
        .caption("Vertical Boxplot", ("sans-serif", 20u32))
        .build_cartesian_2d(
            LABELS[..].into_segmented(),
            values_range.start..values_range.end + 1.0,
        )?;

    chart.configure_mesh().light_line_style(&WHITE).draw()?;

    chart.draw_series(
        quartiles
            .iter()
            .zip(LABELS.iter())
            .map(|(q, label)| Boxplot::new_vertical(SegmentValue::CenterOf(&*label), q)),
    )?;

    // To avoid the IO failure being ignored silently, we manually call the present function
    root.present().expect("Unable to write result to file, please make sure 'plotters-doc-data' dir exists under current dir");
    println!("Result has been saved to {}", OUT_FILE_NAME);
    Ok(())
}
