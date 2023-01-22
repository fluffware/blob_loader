use nom::{
    branch::alt,
    bytes::complete::{tag, take_until},
    character::complete::{
        alphanumeric1, char as nom_char, digit1, hex_digit1, space0,
    },
    combinator::{map, map_res, opt},
    error::ErrorKind,
    error::FromExternalError,
    error::ParseError,
    multi::{fold_many0, separated_list1},
    sequence::{delimited, pair, preceded, tuple},
    Err, IResult, InputIter, InputTake, Parser,
};

use std::num::ParseIntError;

fn from_hex(input: &str) -> Result<u64, std::num::ParseIntError> {
    u64::from_str_radix(input, 16)
}

fn from_dec(input: &str) -> Result<u64, std::num::ParseIntError> {
    u64::from_str_radix(input, 10)
}

fn hex_number<'a, E>(input: &'a str) -> IResult<&str, u64, E>
where
    E: ParseError<&'a str> + FromExternalError<&'a str, ParseIntError>,
{
    let (input, _) = alt((tag("0x"), tag("0X")))(input)?;
    map_res(hex_digit1, from_hex)(input)
}

fn dec_number<'a, E>(input: &'a str) -> IResult<&str, u64, E>
where
    E: ParseError<&'a str> + FromExternalError<&'a str, ParseIntError>,
{
    map_res(digit1, from_dec)(input)
}

fn number<'a, E>(input: &'a str) -> IResult<&'a str, i64, E>
where
    E: ParseError<&'a str> + FromExternalError<&'a str, ParseIntError>,
{
    alt((
        map(
            tuple((alt((nom_char('-'), nom_char('+'))), number)),
            |(s, n)| {
                if s == '-' {
                    -n
                } else {
                    n
                }
            },
        ),
        map(alt((hex_number, dec_number)), |a| a as i64),
    ))(input)
}

fn suffixed<'a, E>(input: &'a str) -> IResult<&str, i64, E>
where
    E: ParseError<&'a str> + FromExternalError<&'a str, ParseIntError>,
{
    map(
        tuple((number, opt(alt((nom_char('K'), nom_char('M')))))),
        |(v, s)| {
            if let Some(s) = s {
                if s == 'K' {
                    v * 1024
                } else {
                    v * (1024 * 1024)
                }
            } else {
                v
            }
        },
    )(input)
}

fn term<'a, E>(input: &'a str) -> IResult<&'a str, i64, E>
where
    E: ParseError<&'a str> + FromExternalError<&'a str, ParseIntError>,
{
    alt((
        delimited(
            tuple((nom_char('('), space0)),
            terms,
            tuple((space0, nom_char(')'))),
        ),
        map(preceded(tuple((nom_char('-'), space0)), term), |a| -a),
        suffixed,
    ))(input)
}

fn terms<'a, E>(input: &'a str) -> IResult<&'a str, i64, E>
where
    E: ParseError<&'a str> + FromExternalError<&'a str, ParseIntError>,
{
    let (input, first) = term(input)?;
    fold_many0(
        tuple((
            preceded(space0, alt((nom_char('+'), nom_char('-')))),
            preceded(space0, term),
        )),
        move || first,
        |a: i64, (op, b)| {
            if op == '+' {
                a + b
            } else {
                a - b
            }
        },
    )(input)
}

fn expr<'a, E>(input: &'a str) -> IResult<&'a str, i64, E>
where
    E: ParseError<&'a str> + FromExternalError<&'a str, ParseIntError>,
{
    terms(input)
}

#[derive(PartialEq, Debug)]
pub struct LinkParseError<'a> {
    input: &'a str,
    kind: LinkParseErrorKind,
}

#[derive(PartialEq, Debug)]
pub enum LinkParseErrorKind {
    ParseError(ErrorKind),
    ParseInt(ErrorKind, ParseIntError),
    MissingOrigin,
    MissingLength,
    IncorrectRegion,
}

impl<'a> ParseError<&'a str> for LinkParseError<'a> {
    fn from_error_kind(input: &'a str, kind: ErrorKind) -> Self {
        LinkParseError {
            input,
            kind: LinkParseErrorKind::ParseError(kind),
        }
    }

    fn append(_input: &str, _kind: ErrorKind, other: Self) -> Self {
        other
    }
}

impl<'a> FromExternalError<&'a str, ParseIntError> for LinkParseError<'a> {
    fn from_external_error(input: &'a str, kind: ErrorKind, err: ParseIntError) -> Self {
        LinkParseError {
            input,
            kind: LinkParseErrorKind::ParseInt(kind, err),
        }
    }
}

pub fn take_till_and_consume<'a, I, P, E, G>(mut g: G) -> impl FnMut(I) -> IResult<I, (I, P), E>
where
    I: InputTake + Clone + InputIter + std::fmt::Display,
    G: Parser<I, P, E>,
    E: ParseError<I>,
{
    move |input: I| {
        for (i, _) in input.iter_indices() {
            let (after, before) = input.take_split(i);
            match g.parse(after) {
                Ok((rest, res)) => return Ok((rest, (before, res))),
                Err(Err::Error(_)) => {}
                Err(e) => return Err(e),
            }
        }
        Err(Err::Error(E::from_error_kind(input, ErrorKind::TakeTill1)))
    }
}

fn memory_arg(input: &str) -> IResult<&str, (&str, i64), LinkParseError> {
    let (input, (_, name, _, _, _, value)) =
        tuple((space0, alphanumeric1, space0, tag("="), space0, expr))(input)?;
    Ok((input, (name, value)))
}

fn memory_line(input: &str) -> IResult<&str, (&str, Option<&str>, i64, i64), LinkParseError> {
    let (input, name) = delimited(space0, alphanumeric1, space0)(input)?;
    let (input, attr) = opt(delimited(tag("("), take_until(")"), tag(")")))(input)?;
    let (input, _) = tag(":")(input)?;

    let (input, args) = separated_list1(pair(space0, nom_char(',')), memory_arg)(input)?;
    let mut origin = None;
    let mut length = None;
    for a in args {
        match a {
            ("ORIGIN", o) => origin = Some(o),
            ("LENGTH", l) => length = Some(l),
            _ => {}
        }
    }
    let Some(origin) = origin else {
        return Err(nom::Err::Failure(LinkParseError{input, kind: LinkParseErrorKind::MissingOrigin}))
    };
    let Some(length) = length else {
        return Err(nom::Err::Failure(LinkParseError{input, kind: LinkParseErrorKind::MissingLength}))
    };
    Ok((input, (name, attr, origin, length)))
}

fn named_memory_line<'a>(
    input: &'a str,
    match_name: &str,
) -> IResult<&'a str, (&'a str, Option<&'a str>, i64, i64), LinkParseError<'a>> {
    match memory_line(input) {
        Ok((input, (name, attr, origin, length))) => {
            if name == match_name {
                Ok((input, (name, attr, origin, length)))
            } else {
                Err(Err::Error(LinkParseError{input, kind: LinkParseErrorKind::IncorrectRegion}))
            }
        }
        Err(e) => Err(e),
    }
}

pub fn find_memory_def<'a>(
    input: &'a str,
    name: &str,
) -> IResult<&'a str, (&'a str, (&'a str, Option<&'a str>, i64, i64)), LinkParseError<'a>> {
    take_till_and_consume(|input| named_memory_line(input, name))(input)
}

#[test]
fn test_parse_number() {
    assert_eq!(number::<nom::error::Error<_>>("89"), Ok(("", 89)));
    assert_eq!(number::<nom::error::Error<_>>("0xaa9"), Ok(("", 0xaa9)));
}

#[test]
fn test_parse_memory() {
    assert_eq!(
        memory("ddfshj MEMORY\n{\nFLASH: ORIGIN=0, LENGTH = 2K}\n"),
        Ok(("\n", (vec! {("FLASH",None, 0,2048)})))
    );
    assert_eq!(
        memory(
            r#"
MEMORY {
    BOOT2 : ORIGIN = 0x10000000, LENGTH = 0x100
    FLASH : ORIGIN = 0x10000100, LENGTH = 1024K - 0x100
    RAM   : ORIGIN = 0x20000000, LENGTH = 256K
}
"#
        ),
        Ok((
            "\n",
            (vec! {("BOOT2",None, 0x10000000, 0x100), ("FLASH",None, 0x10000100, 1024*1024-0x100), ("RAM", None, 0x20000000, 256*1024)})
        ))
    );
}

#[test]
fn test_terms() {
    assert_eq!(terms::<nom::error::Error<_>>("3+7-0xa"), Ok(("", 0)));
    assert_eq!(terms::<nom::error::Error<_>>("3 +7 - 0xa"), Ok(("", 0)));
    assert_eq!(terms::<nom::error::Error<_>>("3 +7 - 0xa"), Ok(("", 0)));
    assert_eq!(terms::<nom::error::Error<_>>("-3 +7 - 0xa"), Ok(("", -6)));
    assert_eq!(terms::<nom::error::Error<_>>("-3 -(7 - 0xa)"), Ok(("", 0)));
    assert_eq!(terms::<nom::error::Error<_>>("-(-(7))"), Ok(("", 7)));
    assert_eq!(terms::<nom::error::Error<_>>("-8"), Ok(("", -8)));
    assert_eq!(terms::<nom::error::Error<_>>("-8--9"), Ok(("", 1)));
}

#[test]
fn test_expr() {
    assert_eq!(
        expr::<nom::error::Error<_>>("3+7K-0xa"),
        Ok(("", 3 + 7 * 1024 - 0xa))
    );
    assert_eq!(expr::<nom::error::Error<_>>("0MK"), Ok(("K", 0)));
    assert_eq!(expr::<nom::error::Error<_>>("1MK"), Ok(("K", 1024 * 1024)));
}

#[test]
fn test_take_till_and_consume() {
    let res: IResult<&str, (&str, &str)> = take_till_and_consume(tag("o"))("fog");
    assert_eq!(res, Ok(("g", ("f", "o"))));
    let res: IResult<&str, (&str, &str)> = take_till_and_consume(tag("fo"))("fog");
    assert_eq!(res, Ok(("g", ("", "fo"))));

    let res: IResult<&str, ((&str, &str), &str)> =
        tuple((take_till_and_consume(tag("fo")), space0))("asfo  g");
    assert_eq!(res, Ok(("g", (("as", "fo"), "  "))));

    let res: IResult<&str, (&str, &str)> = take_till_and_consume(tag("hk"))("ikdks");
    if let Err(ref e) = res {
        println!("Err: {}", e);
    }
}
