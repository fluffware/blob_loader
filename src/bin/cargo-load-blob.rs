fn main()
{
    for (name, value) in std::env::vars() {
        println!("{} = {}", name, value);
    }
}
