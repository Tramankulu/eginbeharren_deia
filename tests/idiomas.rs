//! Tabla de idiomas: todas las filas completas y las plantillas coherentes.

use deia::lang;

fn placeholders(s: &str) -> usize {
    let mut n = 0;
    let mut rest = s;
    while let Some(i) = rest.find('{') {
        let after = &rest[i + 1..];
        match after.find('}') {
            Some(j) if after[..j].chars().all(|c| c.is_ascii_digit()) => {
                n += 1;
                rest = &after[j + 1..];
            }
            _ => rest = after,
        }
    }
    n
}

#[test]
fn tabla_completa() {
    let rows = lang::rows();
    assert!(rows.len() > 200, "faltan frases: {}", rows.len());
    let mut keys = std::collections::HashSet::new();
    for r in &rows {
        assert_eq!(r.len(), 10, "fila con {} columnas: {:?}", r.len(), r[0]);
        assert!(keys.insert(r[0]), "frase repetida: {}", r[0]);
        let n = placeholders(r[0]);
        for (k, t) in r.iter().enumerate().skip(1) {
            // Solo el valenciano puede quedar vacío (usa el catalán)
            if t.is_empty() {
                assert_eq!(k, 8, "casilla vacía en {:?} (columna {k})", r[0]);
                continue;
            }
            assert_eq!(placeholders(t), n, "datos distintos en {:?} → {:?}", r[0], t);
        }
    }
}

#[test]
fn traducciones() {
    lang::set("eu");
    assert_eq!(lang::tr("Salir"), "Irten");
    assert_eq!(lang::tr("Oleada 3"), "3. olatua");
    // Con espacios alrededor y datos traducidos a su vez
    assert_eq!(lang::tr(" se ha suicidado"), " bere burua hil du");
    assert_eq!(lang::tr("Tu equipo: Aliados"), "Zure taldea: Aliatuak");
    assert_eq!(lang::tr("El Eje ha sido eliminado. Ronda para los Aliados."), "Ardatza suntsitua izan da. Txanda Aliatuentzat.");
    assert_eq!(lang::tr("Bot3 se une al Eje"), "Bot3 Ardatzarekin bat egin du");
    // Lo que no está en la tabla se queda igual
    assert_eq!(lang::tr("Kar98k"), "Kar98k");
    lang::set("val");
    assert_eq!(lang::tr("Salir"), "Eixir");
    assert_eq!(lang::tr("Conectar"), "Connectar"); // casilla vacía: catalán
    lang::set("ru");
    assert_eq!(lang::tr("Siguiente ronda en 5"), "Следующий раунд через 5");
    lang::set("es");
    assert_eq!(lang::tr("Salir"), "Salir");
    lang::set("eu");
}
