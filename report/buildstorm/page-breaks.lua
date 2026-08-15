local seen_title = false
local seen_first_chapter = false
local in_main_report = true

function Header(header)
  if header.level == 1 then
    if not seen_title then
      seen_title = true
      -- Pandoc emits the outline before the body, so the first break keeps the
      -- report title and abstract off the final contents page.
      return { pandoc.RawBlock("typst", "#pagebreak()"), header }
    end
    -- Each appended research document starts with a level-one heading.
    in_main_report = false
    return { pandoc.RawBlock("typst", "#pagebreak()"), header }
  end

  -- The main Markdown file uses level two for chapters. Once the first
  -- appended document begins, level-two headings are ordinary subsections.
  if in_main_report and header.level == 2 then
    if not seen_first_chapter then
      seen_first_chapter = true
      return header
    end
    return { pandoc.RawBlock("typst", "#pagebreak()"), header }
  end

  return header
end
