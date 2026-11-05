//! The window from the outside (F39, F40, F02, F22): panels moved with the mouse, links in
//! Markdown, colours and models, in a real PTY.

mod common;

#[cfg(unix)]
use common::COLUMNS;
use common::{Home, Sozo};

/// SozoIDE with both terminals open.
#[cfg(unix)]
fn with_all_panels(home: &Home) -> Sozo {
    let sozo = home.start();
    sozo.past_splash();
    sozo.send(b"\n");
    sozo.wait_for("Bottom terminal");
    // Back to the editor, whose keys open the right terminal.
    sozo.send(b"\x1b[17~");
    let key: &[u8] = if cfg!(target_os = "windows") {
        b"\x02"
    } else {
        b"\x1b\x02"
    };
    sozo.send(key);
    sozo.wait_for("Right terminal");
    sozo.send(b"\x1b[17~");
    std::thread::sleep(std::time::Duration::from_millis(500));
    sozo
}

#[cfg(unix)]
#[test]
fn the_explorer_moves_to_a_side_of_a_terminal_and_the_editor_takes_its_room() {
    let home = Home::new();
    let sozo = with_all_panels(&home);
    let (row, column) = sozo.find("Explorer");
    assert!(column < 5);
    let right = sozo.find("Right terminal");
    sozo.drag((row, column), (right.0 + 2, right.1 + 8));
    sozo.wait_until("the Explorer on the right", |_| {
        sozo.locate("Explorer")
            .is_some_and(|(_, column)| column > COLUMNS / 2)
    });
    let moved = sozo.find("Explorer");
    let terminal = sozo.find("Right terminal");
    assert!(terminal.0 > moved.0, "above the terminal");
    assert!(terminal.1.abs_diff(moved.1) < 4);
    sozo.quit();
}

#[cfg(unix)]
#[test]
fn a_panel_dropped_on_the_edge_takes_it_and_one_on_a_centre_swaps() {
    let home = Home::new();
    let sozo = with_all_panels(&home);
    // The bottom terminal over the whole top of the window.
    let bottom = sozo.find("Bottom terminal");
    sozo.drag(bottom, (1, COLUMNS / 2));
    sozo.wait_until("the terminal on top", |_| {
        sozo.locate("Bottom terminal")
            .is_some_and(|(row, _)| row <= 2)
    });
    let top = sozo.find("Bottom terminal");
    assert!(top.1 < 5, "from the first column: {top:?}");
    // The right terminal with the Explorer, by the centre of the Explorer.
    let explorer = sozo.find("Explorer");
    let right = sozo.find("Right terminal");
    sozo.drag(right, (explorer.0 + 15, explorer.1 + 12));
    sozo.wait_until("the two swapped", |_| {
        sozo.locate("Right terminal")
            .is_some_and(|(_, column)| column < 8)
    });
    assert!(sozo.find("Explorer").1 > COLUMNS / 2);
    sozo.quit();
}

#[cfg(unix)]
#[test]
fn the_editor_moves_by_the_free_part_of_its_tab_bar() {
    let home = Home::new();
    let sozo = with_all_panels(&home);
    let right = sozo.find("Right terminal");
    let explorer = sozo.find("Explorer");
    // No tabs yet: the first row of the editor, after the Explorer.
    let from = (explorer.0, explorer.1 + 40);
    sozo.drag(from, (right.0 + 12, right.1 + 8));
    // Swapped with the right terminal: the terminal sits where the editor was.
    sozo.wait_until("the terminal on the left", |_| {
        sozo.locate("Right terminal")
            .is_some_and(|(_, column)| column < COLUMNS / 2)
    });
    sozo.quit();
}

#[cfg(unix)]
#[test]
fn a_click_on_a_title_moves_nothing_and_a_divider_resizes_on_its_side() {
    let home = Home::new();
    let sozo = with_all_panels(&home);
    let right = sozo.find("Right terminal");
    sozo.click(right);
    assert_eq!(sozo.find("Right terminal"), right);
    // The divider on the left of the right terminal, ten columns to the left.
    let divider = (right.0 + 5, right.1 - 5);
    sozo.drag(divider, (divider.0, divider.1 - 10));
    sozo.wait_until("a wider terminal", |_| {
        sozo.locate("Right terminal")
            .is_some_and(|(_, column)| column + 10 == right.1)
    });
    sozo.quit();
}

#[test]
fn links_show_their_text_and_open_their_documents_once() {
    let home = Home::new();
    home.write(
        "index.md",
        "# Top\n\nSee [[guide|the guide]] and [[other]] here\n\nlast line\n",
    );
    home.write("guide.md", "GUIDE_BODY\n");
    home.write("other.md", "OTHER_BODY\n");
    let sozo = home.start();
    sozo.past_splash();
    sozo.click(sozo.find("index.md"));
    // The cursor is on the first row, so the row with the links shows only their text.
    sozo.wait_for("See the guide and other here");
    assert!(!sozo.screen().contains("[["));
    let link = sozo.find("the guide");
    sozo.send(format!("\x1b[<35;{};{}M", link.1 + 2, link.0 + 1));
    sozo.click((link.0, link.1 + 1));
    sozo.wait_for("GUIDE_BODY");

    // Back on the first document, the link goes to the tab that is open.
    let tab = sozo.find("index.md");
    let tabs = sozo
        .rows()
        .iter()
        .filter(|row| row.contains("guide.md"))
        .count();
    sozo.click((tab.0, tab.1));
    sozo.wait_for("See the guide");
    let tab_row = sozo.rows()[usize::from(tab.0)].clone();
    let link = sozo.find("the guide");
    sozo.click((link.0, link.1 + 1));
    sozo.wait_for("GUIDE_BODY");
    let row = &sozo.rows()[usize::from(tab.0)];
    assert_eq!(
        row.matches("guide.md").count(),
        tab_row.matches("guide.md").count()
    );
    assert_eq!(
        sozo.rows()
            .iter()
            .filter(|row| row.contains("guide.md"))
            .count(),
        tabs
    );
    sozo.quit();
}

#[test]
fn a_link_without_a_document_says_so() {
    let home = Home::new();
    home.write("index.md", "# Top\n\nSee [[nothing here]] now\n");
    let sozo = home.start();
    sozo.past_splash();
    sozo.click(sozo.find("index.md"));
    sozo.wait_for("nothing here");
    let link = sozo.find("nothing here");
    sozo.click((link.0, link.1 + 1));
    sozo.wait_for("No document for [[nothing here]]");
    sozo.quit();
}

fn coloured(sozo: &Sozo, text: &str) -> bool {
    let (row, column) = sozo.find(text);
    sozo.colour(row, column) != vt100::Color::Default
}

#[test]
fn astro_and_eclipse_files_are_coloured() {
    let home = Home::new();
    home.write(
        "page.astro",
        "---\nconst title = \"Sozo\";\n---\n<h1 class=\"big\">{title}</h1>\n",
    );
    home.write(
        ".classpath",
        "<classpath>\n  <entry kind=\"src\"/>\n</classpath>\n",
    );
    let sozo = home.start();
    sozo.past_splash();
    sozo.click(sozo.find("page.astro"));
    // The file comes in line by line.
    sozo.wait_for("</h1>");
    assert!(coloured(&sozo, "const"), "the script");
    assert!(coloured(&sozo, "h1"), "the template");
    sozo.wait_for("Astro");
    sozo.click(sozo.find(".classpath"));
    sozo.wait_for("</classpath>");
    assert!(coloured(&sozo, "entry"));
    sozo.quit();
}

/// A `.glb` with a triangle, and `extra` members in its JSON.
fn glb(extra: &str) -> Vec<u8> {
    let json = format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{{"mesh":0}}],"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"material":0}}]}}],"materials":[{{"pbrMetallicRoughness":{{"baseColorFactor":[1,0,0,1]}}}}],"buffers":[{{"byteLength":36}}],"bufferViews":[{{"buffer":0,"byteLength":36}}],"accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[-1,-1,0],"max":[1,1,0]}}]{extra}}}"#
    );
    let mut json = json.into_bytes();
    json.resize(json.len().next_multiple_of(4), b' ');
    let binary = [-1.0_f32, -1.0, 0.0, 1.0, -1.0, 0.0, 0.0, 1.0, 0.0]
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect::<Vec<_>>();
    let total = 12 + 8 + json.len() + 8 + binary.len();
    let mut file = b"glTF".to_vec();
    file.extend(2_u32.to_le_bytes());
    file.extend((total as u32).to_le_bytes());
    file.extend((json.len() as u32).to_le_bytes());
    file.extend(b"JSON");
    file.extend(json);
    file.extend((binary.len() as u32).to_le_bytes());
    file.extend(b"BIN\0");
    file.extend(binary);
    file
}

#[test]
fn models_open_also_with_an_extension_and_a_texture_nobody_knows() {
    let home = Home::new();
    home.write("plain.glb", glb(""));
    home.write(
        "exported.glb",
        glb(r#","extensionsUsed":["MSFT_texture_dds"],"extensionsRequired":["MSFT_texture_dds"],"images":[{"mimeType":"image/vnd-ms.dds","uri":"data:image/vnd-ms.dds;base64,AAAA"}],"textures":[{"source":0}]"#),
    );
    home.write("broken.glb", b"glTF is not this");
    let sozo = home.start();
    sozo.past_splash();
    sozo.click(sozo.find("exported.glb"));
    sozo.wait_for("turn");
    sozo.click(sozo.find("plain.glb"));
    sozo.wait_until("the second tab", |screen| {
        screen.matches("plain.glb").count() >= 2
    });
    // A file that is no model says so, and the app stays.
    sozo.click(sozo.find("broken.glb"));
    sozo.wait_for("Could not open");
    sozo.quit();
}
