#!/usr/bin/env elixir

# Simulate the first anneal-code Elixir adapter layer from EEP-48 Docs chunks.
# The script ingests compiled artifacts only; it does not compile the project.

defmodule Eep48Sim do
  @moduledoc false

  def main(argv) do
    argv
    |> parse_args()
    |> run()
  rescue
    error ->
      IO.puts(:stderr, "error: #{Exception.message(error)}")
      System.halt(1)
  end

  defp parse_args(argv) do
    defaults = %{
      app: nil,
      build_env: "dev",
      output: nil,
      root: nil
    }

    parse_args(argv, defaults)
  end

  defp parse_args([], opts) do
    require_arg!(opts, :root, "--root")
    require_arg!(opts, :app, "--app")
    require_arg!(opts, :output, "--output")
    opts
  end

  defp parse_args(["--root", value | rest], opts), do: parse_args(rest, %{opts | root: value})
  defp parse_args(["--app", value | rest], opts), do: parse_args(rest, %{opts | app: value})
  defp parse_args(["--build-env", value | rest], opts), do: parse_args(rest, %{opts | build_env: value})
  defp parse_args(["--output", value | rest], opts), do: parse_args(rest, %{opts | output: value})

  defp parse_args(["--help" | _], _opts) do
    IO.puts("""
    Usage:
      scripts/simulate-eep48-extraction.exs --root PATH --app APP --output PATH [--build-env dev]

    Reads BEAM EEP-48 Docs chunks from _build/ENV/lib/APP/ebin and optional
    doc/chunks fallback files. Emits aggregate JSON for adapter-design evidence.
    """)

    System.halt(0)
  end

  defp parse_args([unknown | _], _opts), do: raise("unknown argument #{unknown}")

  defp require_arg!(opts, key, flag) do
    if Map.fetch!(opts, key) in [nil, ""] do
      raise("missing required #{flag}")
    end
  end

  defp run(opts) do
    root = Path.expand(opts.root)
    app = opts.app
    ebin = Path.join([root, "_build", opts.build_env, "lib", app, "ebin"])
    chunks_dir = Path.join([root, "_build", opts.build_env, "lib", app, "doc", "chunks"])

    beam_paths = Path.wildcard(Path.join(ebin, "*.beam")) |> Enum.sort()
    chunk_paths = Path.wildcard(Path.join(chunks_dir, "*.chunk")) |> Enum.sort()

    add_code_paths(root, opts.build_env)

    modules =
      beam_paths
      |> Enum.map(&inspect_beam(&1, chunks_dir))
      |> Enum.sort_by(& &1.module)

    source_counts = source_scan(root)
    aggregates = aggregate(modules, beam_paths, chunk_paths, source_counts, root, opts)

    output = %{
      "input" => %{
        "app" => app,
        "build_env" => opts.build_env,
        "beam_count" => length(beam_paths),
        "external_doc_chunk_count" => length(chunk_paths),
        "build_missing" => beam_paths == []
      },
      "aggregates" => aggregates,
      "modules" => Enum.map(modules, &module_row/1)
    }

    opts.output
    |> Path.expand()
    |> File.write!(JSON.encode!(output))

    IO.puts("wrote #{opts.output}")
  end

  defp add_code_paths(root, build_env) do
    root
    |> Path.join(Path.join(["_build", build_env, "lib", "*", "ebin"]))
    |> Path.wildcard()
    |> Enum.each(fn path -> :code.add_patha(String.to_charlist(path)) end)
  end

  defp inspect_beam(path, chunks_dir) do
    module = beam_module(path)
    source = docs_source(path, module, chunks_dir)
    docs = source.docs
    docs_entries = docs_entries(docs)
    metadata = docs_metadata(docs)
    moduledoc = module_doc(docs)

    %{
      path: path,
      module: inspect(module),
      source_path: source_path(metadata),
      docs_source: source.source,
      docs_available: docs != nil,
      docs_format: docs_format(docs),
      module_doc_state: doc_state(moduledoc),
      module_doc_bytes: doc_bytes(moduledoc),
      module_metadata: normalize_metadata(metadata),
      entries: Enum.map(docs_entries, &entry_info/1),
      specs: spec_info(module),
      callbacks: callback_info(module)
    }
  end

  defp beam_module(path) do
    case :beam_lib.info(String.to_charlist(path))[:module] do
      nil -> raise("cannot read module from #{path}")
      module -> module
    end
  end

  defp docs_source(path, module, chunks_dir) do
    case read_beam_docs(path) do
      {:ok, docs} ->
        %{source: "beam_docs_chunk", docs: docs}

      :missing ->
        case read_external_chunk(module, chunks_dir) do
          {:ok, docs} -> %{source: "external_doc_chunk", docs: docs}
          :missing -> %{source: "missing_or_stripped", docs: nil}
        end
    end
  end

  defp read_beam_docs(path) do
    case :beam_lib.chunks(String.to_charlist(path), [~c"Docs"]) do
      {:ok, {_, [{~c"Docs", bin}]}} -> {:ok, :erlang.binary_to_term(bin)}
      _ -> :missing
    end
  end

  defp read_external_chunk(module, chunks_dir) do
    basename = module |> inspect() |> Kernel.<>(".chunk")
    path = Path.join(chunks_dir, basename)

    case File.read(path) do
      {:ok, bin} -> {:ok, :erlang.binary_to_term(bin)}
      {:error, _} -> :missing
    end
  end

  defp docs_entries(nil), do: []
  defp docs_entries({:docs_v1, _, _, _, _, _, entries}), do: entries

  defp docs_metadata(nil), do: %{}
  defp docs_metadata({:docs_v1, _, _, _, _, metadata, _}), do: metadata

  defp docs_format(nil), do: nil
  defp docs_format({:docs_v1, _, _, format, _, _, _}), do: to_string(format)

  defp module_doc(nil), do: nil
  defp module_doc({:docs_v1, _, _, _, moduledoc, _, _}), do: moduledoc

  defp source_path(metadata) do
    metadata
    |> Map.get(:source_path, Map.get(metadata, "source_path"))
    |> case do
      nil -> nil
      value -> to_string(value)
    end
  end

  defp entry_info({{kind, name, arity}, anno, signature, doc, metadata}) do
    %{
      kind: to_string(kind),
      name: to_string(name),
      arity: arity,
      line: annotation_line(anno),
      signatures: Enum.map(signature || [], &to_string/1),
      doc_state: doc_state(doc),
      doc_bytes: doc_bytes(doc),
      metadata: normalize_metadata(metadata),
      link_targets: doc_links(doc)
    }
  end

  defp annotation_line(line) when is_integer(line), do: line
  defp annotation_line({line, _column}) when is_integer(line), do: line
  defp annotation_line(_), do: nil

  defp doc_state(nil), do: "missing"
  defp doc_state(:hidden), do: "hidden"
  defp doc_state(:none), do: "none"
  defp doc_state(doc) when is_map(doc), do: "documented"
  defp doc_state(_), do: "unknown"

  defp doc_bytes(doc) when is_map(doc) do
    doc
    |> Map.values()
    |> Enum.map(&text_bytes/1)
    |> Enum.sum()
  end

  defp doc_bytes(_), do: 0

  defp text_bytes(text) when is_binary(text), do: byte_size(text)
  defp text_bytes(text), do: text |> inspect() |> byte_size()

  defp doc_links(doc) when is_map(doc) do
    doc
    |> Map.values()
    |> Enum.flat_map(fn text ->
      ~r/\[[^\]]+\]\(([^)]+)\)/
      |> Regex.scan(to_string(text), capture: :all_but_first)
      |> Enum.map(fn [target] -> target end)
    end)
  end

  defp doc_links(_), do: []

  defp normalize_metadata(metadata) when is_map(metadata) do
    metadata
    |> Enum.map(fn {key, value} -> {to_string(key), metadata_value(value)} end)
    |> Map.new()
  end

  defp metadata_value(value) when is_binary(value), do: value
  defp metadata_value(value) when is_boolean(value), do: value
  defp metadata_value(value) when is_atom(value), do: to_string(value)
  defp metadata_value(value) when is_integer(value), do: value
  defp metadata_value(value) when is_list(value), do: Enum.map(value, &metadata_value/1)
  defp metadata_value(value), do: inspect(value)

  defp spec_info(module) do
    case Code.Typespec.fetch_specs(module) do
      {:ok, specs} -> Enum.map(specs, &typespec_entry/1)
      :error -> []
    end
  rescue
    _ -> []
  end

  defp callback_info(module) do
    case Code.Typespec.fetch_callbacks(module) do
      {:ok, specs} -> Enum.map(specs, &typespec_entry/1)
      :error -> []
    end
  rescue
    _ -> []
  end

  defp typespec_entry({{name, arity}, specs}) do
    refs =
      specs
      |> Enum.flat_map(&type_refs/1)
      |> Enum.uniq()
      |> Enum.sort()

    %{
      name: to_string(name),
      arity: arity,
      refs: refs
    }
  end

  defp type_refs(term) do
    refs =
      case term do
        {:remote_type, _, [{:atom, _, module}, {:atom, _, name}, _args]} ->
          ["#{inspect(module)}.#{name}"]

        {:user_type, _, name, _args} ->
          ["local:#{name}"]

        _ ->
          []
      end

    refs ++
      cond do
        is_tuple(term) -> term |> Tuple.to_list() |> Enum.flat_map(&type_refs/1)
        is_list(term) -> Enum.flat_map(term, &type_refs/1)
        true -> []
      end
  end

  defp source_scan(root) do
    source_files = Path.wildcard(Path.join([root, "{lib,test}", "**", "*.{ex,exs}"]))

    Enum.reduce(
      source_files,
      %{
        "source_files" => length(source_files),
        "source_bytes" => 0,
        "defimpl_mentions" => 0,
        "behaviour_mentions" => 0,
        "typespec_mentions" => 0,
        "deprecated_mentions" => 0,
        "since_mentions" => 0
      },
      fn path, acc ->
        text = File.read!(path)

        acc
        |> inc("source_bytes", byte_size(text))
        |> inc("defimpl_mentions", count_regex(text, ~r/\bdefimpl\b/))
        |> inc("behaviour_mentions", count_regex(text, ~r/@behaviour\b/))
        |> inc("typespec_mentions", count_regex(text, ~r/@(spec|callback|type|typep|opaque)\b/))
        |> inc("deprecated_mentions", count_regex(text, ~r/@deprecated\b|deprecated:/))
        |> inc("since_mentions", count_regex(text, ~r/@since\b|since:/))
      end
    )
  end

  defp count_regex(text, regex), do: Regex.scan(regex, text) |> length()
  defp inc(map, key, amount), do: Map.update!(map, key, &(&1 + amount))

  defp aggregate(modules, beam_paths, chunk_paths, source_counts, root, opts) do
    entries = Enum.flat_map(modules, & &1.entries)
    specs = Enum.flat_map(modules, & &1.specs)
    callbacks = Enum.flat_map(modules, & &1.callbacks)
    type_refs = Enum.flat_map(specs ++ callbacks, & &1.refs)

    module_sources =
      modules
      |> Enum.group_by(& &1.source_path)
      |> Map.delete(nil)

    multi_module_files =
      module_sources
      |> Enum.filter(fn {_path, rows} -> length(rows) > 1 end)
      |> length()

    doc_links = Enum.flat_map(entries, & &1.link_targets)

    %{
      "artifact" => %{
        "app" => opts.app,
        "build_env" => opts.build_env,
        "build_missing" => beam_paths == [],
        "beams" => length(beam_paths),
        "external_doc_chunks_available" => length(chunk_paths),
        "root_basename" => Path.basename(root)
      },
      "degraded_cases" => %{
        "beam_docs_chunk" => Enum.count(modules, &(&1.docs_source == "beam_docs_chunk")),
        "external_doc_chunk_fallback" => Enum.count(modules, &(&1.docs_source == "external_doc_chunk")),
        "missing_or_stripped" => Enum.count(modules, &(&1.docs_source == "missing_or_stripped")),
        "build_missing" => beam_paths == []
      },
      "scale" => %{
        "projected_handles" => length(modules) + length(entries),
        "modules" => length(modules),
        "members" => length(entries),
        "projected_edges" => projected_edge_count(modules, entries, doc_links, type_refs),
        "containment_edges" => length(entries),
        "uses_type_edges" => length(type_refs),
        "doc_link_edges" => length(doc_links),
        "implements_edges" => implements_edge_count(modules, source_counts),
        "doc_bytes" => Enum.map(modules, & &1.module_doc_bytes) |> Enum.sum(),
        "member_doc_bytes" => Enum.map(entries, & &1.doc_bytes) |> Enum.sum(),
        "signature_bytes" => signature_bytes(entries)
      },
      "item_kinds" => frequencies(Enum.map(entries, & &1.kind)),
      "module_doc_states" => frequencies(Enum.map(modules, & &1.module_doc_state)),
      "member_doc_states" => frequencies(Enum.map(entries, & &1.doc_state)),
      "metadata_coverage" => metadata_coverage(modules, entries),
      "typespec_coverage" => %{
        "spec_entries" => length(specs),
        "callback_entries" => length(callbacks),
        "member_entries_with_spec" => count_members_with_spec(entries, specs),
        "member_entries" => length(entries),
        "type_ref_edges" => length(type_refs),
        "unique_type_refs" => length(Enum.uniq(type_refs)),
        "top_type_refs" => top_counts(type_refs, 12)
      },
      "doc_links" => %{
        "markdown_links" => length(doc_links),
        "unique_targets" => length(Enum.uniq(doc_links)),
        "top_targets" => top_counts(doc_links, 12)
      },
      "identity_stressors" => %{
        "source_files_with_modules" => map_size(module_sources),
        "multi_module_files" => multi_module_files,
        "modules_in_multi_module_files" =>
          module_sources
          |> Enum.filter(fn {_path, rows} -> length(rows) > 1 end)
          |> Enum.map(fn {_path, rows} -> length(rows) end)
          |> Enum.sum()
      },
      "source_scan" => source_counts
    }
  end

  defp projected_edge_count(modules, entries, doc_links, type_refs) do
    length(entries) + length(doc_links) + length(type_refs) + implements_edge_count(modules, %{})
  end

  defp implements_edge_count(modules, source_counts) do
    metadata_behaviours =
      modules
      |> Enum.flat_map(fn module ->
        module.module_metadata
        |> Map.get("behaviours", [])
        |> List.wrap()
      end)
      |> length()

    metadata_behaviours + Map.get(source_counts, "defimpl_mentions", 0)
  end

  defp signature_bytes(entries) do
    entries
    |> Enum.flat_map(& &1.signatures)
    |> Enum.map(&byte_size/1)
    |> Enum.sum()
  end

  defp metadata_coverage(modules, entries) do
    module_metadata = Enum.map(modules, & &1.module_metadata)
    entry_metadata = Enum.map(entries, & &1.metadata)

    %{
      "module_since" => count_key(module_metadata, "since"),
      "member_since" => count_key(entry_metadata, "since"),
      "module_deprecated" => count_key(module_metadata, "deprecated"),
      "member_deprecated" => count_key(entry_metadata, "deprecated"),
      "module_hidden_docs" => Enum.count(modules, &(&1.module_doc_state == "hidden")),
      "member_hidden_docs" => Enum.count(entries, &(&1.doc_state == "hidden")),
      "module_behaviours" =>
        module_metadata
        |> Enum.map(&(Map.get(&1, "behaviours", []) |> List.wrap() |> length()))
        |> Enum.sum()
    }
  end

  defp count_key(rows, key) do
    Enum.count(rows, fn row ->
      Map.has_key?(row, key) and Map.get(row, key) not in [nil, "", [], false]
    end)
  end

  defp count_members_with_spec(entries, specs) do
    spec_keys = MapSet.new(Enum.map(specs, &{&1.name, &1.arity}))

    Enum.count(entries, fn entry ->
      MapSet.member?(spec_keys, {entry.name, entry.arity})
    end)
  end

  defp frequencies(values) do
    values
    |> Enum.frequencies()
    |> Enum.sort_by(fn {key, _count} -> key end)
    |> Map.new()
  end

  defp top_counts(values, limit) do
    values
    |> Enum.frequencies()
    |> Enum.sort_by(fn {value, count} -> {-count, value} end)
    |> Enum.take(limit)
    |> Enum.map(fn {value, count} -> %{"value" => value, "count" => count} end)
  end

  defp module_row(module) do
    %{
      "module" => module.module,
      "docs_source" => module.docs_source,
      "docs_available" => module.docs_available,
      "docs_format" => module.docs_format,
      "module_doc_state" => module.module_doc_state,
      "module_doc_bytes" => module.module_doc_bytes,
      "entry_count" => length(module.entries),
      "spec_count" => length(module.specs),
      "callback_count" => length(module.callbacks),
      "behaviour_count" => module.module_metadata |> Map.get("behaviours", []) |> List.wrap() |> length(),
      "source_path" => module.source_path
    }
  end
end

Eep48Sim.main(System.argv())
