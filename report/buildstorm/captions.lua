function Table(table)
  if table.caption and table.caption.long and #table.caption.long > 0 then
    -- Pandoc's Typst writer otherwise emits a table caption as unnumbered
    -- text. A figure wrapper lets Typst infer the table kind and number it.
    local caption = table.caption.long
    table.caption.long = {}
    table.caption.short = nil
    return pandoc.Figure({ table }, caption)
  end
  return table
end
