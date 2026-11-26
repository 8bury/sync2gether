use crate::relay_wire::valid_room;
use std::fmt;

pub const WORDS: [&str; 256] = [
    "agua", "alho", "amora", "anel", "anjo", "arco", "areia", "arroz", "asa", "aveia", "azul",
    "bala", "balde", "banco", "barco", "bico", "bolo", "bola", "bota", "brisa", "cabra", "cacau",
    "caju", "calor", "cama", "campo", "caneca", "canoa", "capa", "carta", "casa", "cavalo",
    "cedro", "cebola", "cereja", "chave", "chuva", "cinema", "circo", "cisne", "coco", "cofre",
    "cola", "cometa", "copo", "coral", "corda", "coruja", "cravo", "cubo", "dado", "dedal",
    "disco", "doce", "dueto", "duna", "eco", "elmo", "escova", "estrela", "faca", "fada", "farol",
    "fava", "feira", "fenda", "ferro", "festa", "figo", "filme", "flauta", "flor", "foca", "fogo",
    "folha", "fonte", "forno", "fruta", "funil", "gaita", "galo", "garfo", "gato", "gelo",
    "girafa", "globo", "goiaba", "gota", "grama", "grilo", "gruta", "guia", "harpa", "hino",
    "horta", "ilha", "janela", "jarra", "jardim", "jipe", "joia", "jogo", "kiwi", "lago", "lama",
    "lanche", "laranja", "lata", "leite", "lenha", "leque", "lima", "lince", "linha", "livro",
    "lobo", "lontra", "lousa", "lua", "luva", "macaco", "mala", "manga", "mapa", "mar", "massa",
    "mel", "menta", "mesa", "milho", "moeda", "mola", "morango", "motor", "mula", "muro", "nabo",
    "navio", "neve", "ninho", "noz", "nuvem", "oca", "ovo", "onda", "ostra", "ouro", "palco",
    "palha", "pano", "papel", "pato", "pedra", "peixe", "pena", "pera", "peteca", "piano", "pilha",
    "pincel", "pinha", "pipa", "pipoca", "pires", "placa", "planta", "pneu", "polvo", "ponte",
    "porta", "pote", "praia", "prato", "queijo", "quiabo", "raio", "ramo", "raposa", "rede",
    "remo", "rio", "roda", "rosa", "roupa", "rua", "sacola", "sal", "salsa", "sapo", "selo",
    "serra", "sino", "sol", "sopa", "tatu", "teia", "telha", "tenda", "terra", "tigre", "tijolo",
    "tinta", "toalha", "tomate", "torre", "trem", "trevo", "trigo", "tucano", "tulipa", "uva",
    "vaca", "vagem", "vale", "vaso", "vela", "vento", "verde", "vidro", "vinho", "viola", "xale",
    "xadrez", "zebra", "baleia", "bambu", "banana", "barro", "batata", "bigode", "bonde", "bosque",
    "broto", "bule", "cacto", "cadeira", "caderno", "camelo", "caneta", "canela", "cesta",
    "chinelo", "colher", "concha", "cupim", "delta", "elefante", "esquilo", "fivela", "formiga",
    "galho", "girassol", "iogurte", "lagarto", "lagoa", "marfim",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Code {
    room: String,
    words: [&'static str; 2],
}
impl Code {
    pub fn new(room: impl Into<String>, words: [&'static str; 2]) -> Self {
        Self {
            room: room.into(),
            words,
        }
    }
    pub fn from_indices(room: impl Into<String>, indices: [u8; 2]) -> Self {
        Self::new(
            room,
            [WORDS[indices[0] as usize], WORDS[indices[1] as usize]],
        )
    }
    pub fn parse(input: &str) -> Option<Self> {
        let folded: String = input.trim().chars().map(fold).collect();
        let parts: Vec<&str> = folded
            .split(|c: char| c == '-' || c == '.' || c.is_whitespace())
            .filter(|part| !part.is_empty())
            .collect();
        let [room, first, second] = parts.as_slice() else {
            return None;
        };
        let word = |input: &str| WORDS.iter().copied().find(|word| *word == input);
        valid_room(room).then_some(())?;
        Some(Self {
            room: (*room).to_owned(),
            words: [word(first)?, word(second)?],
        })
    }
    pub fn room(&self) -> &str {
        &self.room
    }
    pub fn words(&self) -> [&'static str; 2] {
        self.words
    }
}
impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}-{}", self.room, self.words[0], self.words[1])
    }
}
fn fold(c: char) -> char {
    match c.to_lowercase().next().unwrap_or(c) {
        'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'ç' => 'c',
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_unique_short_and_ascii() {
        let mut sorted = WORDS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), WORDS.len());
        assert!(
            WORDS
                .iter()
                .all(|w| (2..=9).contains(&w.len()) && w.bytes().all(|b| b.is_ascii_lowercase()))
        );
    }

    #[test]
    fn codes_roundtrip_and_tolerate_typing_variations() {
        for indices in [[0, 0], [17, 200], [255, 1]] {
            let code = Code::from_indices("4821", indices);
            assert_eq!(Code::parse(&code.to_string()), Some(code.clone()));
        }
        assert_eq!(
            Code::parse("4821-agua-limao"),
            None,
            "palavras fora da lista são recusadas"
        );
        let code = Code::parse("4821-agua-lua").unwrap();
        assert_eq!(code.room(), "4821");
        assert_eq!(code.words(), ["agua", "lua"]);
        for typed in [
            "  4821 Água LUA ",
            "4821-agua-lua",
            "4821.agua.lua",
            "4821 - agua - lua",
        ] {
            assert_eq!(Code::parse(typed).as_ref(), Some(&code), "{typed}");
        }
        for invalid in [
            "",
            "4821",
            "4821-agua",
            "0821-agua-lua",
            "4821-agua-lua-sol",
            "abc-agua-lua",
        ] {
            assert!(Code::parse(invalid).is_none(), "{invalid}");
        }
    }
}
