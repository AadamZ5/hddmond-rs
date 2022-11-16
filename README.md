<!-- @format -->

# hddmond

Back at my first job, I tried to create a bulk-hdd testing software to better support our testing efforts and expedite the process.

Now since `smartmontools` can output in JSON, I feel much more inclined to take another go at this, writing in Rust this time.

## Goals

- [ ] Hot-swap HDD/SSD support
  - [ ] SMART tests
  - [ ] SMART attribute logging / viewing
  - [ ] HDD/SSD shredding
- [ ] USB support
  - [ ] USB sector testing
  - [ ] USB shredding
- [ ] User-defineable "tasks"
  - [ ] Use `deno_core` to provide built-in runtime inside of this program
- [ ] Database support
  - [ ] View how many times a storage device has been inserted/seen
  - [ ] View attribute trends for HDD/SSD
