# frozen_string_literal: true

# Shared helpers for the reproduction scripts: binaries, polygon validation,
# table output, and a parallel job runner with a memory cap.

require 'etc'
require 'fileutils'
require 'json'

module Repro
  ROOT = File.expand_path('../..', __dir__).freeze
  BIN = {
    heuristic: File.join(ROOT, 'heuristic/target/release/lattice-classic-parallel'),
    certify: File.join(ROOT, 'certify/target/release/lattice-layered'),
    modular: File.join(ROOT, 'modular/target/release/lattice-parametric-dp')
  }.freeze

  module_function

  def build!
    %w[heuristic certify modular].each do |crate|
      ok = system('cargo', 'build', '--release', '--locked', '--manifest-path', File.join(ROOT, crate, 'Cargo.toml'))
      abort "build of #{crate} failed" unless ok
    end
  end

  # Vertices of a polygon file: optional '#' header lines, the vertex count,
  # then one "x y" pair per line.
  def read_polygon(path, count)
    lines = File.readlines(path).reject { |l| l.start_with?('#') || l.strip.empty? }
    points = lines.drop(1).map { |l| l.split.map { |w| Integer(w, 10) } }
    abort "#{path}: expected #{count} vertices" unless lines.first.to_i == count && points.size == count
    points
  end

  # Doubled area of a strictly convex counterclockwise polygon, else nil.
  def convex_area2(points)
    size = points.size
    turns = (0...size).map do |i|
      a = points[i]
      b = points[(i + 1) % size]
      c = points[(i + 2) % size]
      ((b[0] - a[0]) * (c[1] - b[1])) - ((b[1] - a[1]) * (c[0] - b[0]))
    end
    return nil unless turns.all?(&:positive?)

    (0...size).sum { |i| (points[i][0] * points[(i + 1) % size][1]) - (points[(i + 1) % size][0] * points[i][1]) }
  end

  def check!(count, area2, points)
    abort "n=#{count}: polygon is not strictly convex or its area is not #{area2}" unless convex_area2(points) == area2
  end

  # rows: [n, doubled area, points]; writes b070911/b063984/a070911 files.
  def write_tables(dir, suffix, header, rows)
    FileUtils.mkdir_p(dir)
    File.write(File.join(dir, "b070911#{suffix}.txt"), header + rows.map { |n, a, _| "#{n} #{a}\n" }.join)
    File.write(File.join(dir, "b063984#{suffix}.txt"), header + rows.map { |n, a, _| "#{n} #{(a - n + 2) / 2}\n" }.join)
    polygons = rows.map { |n, a, pts| "#{n} #{a} #{pts.map { |x, y| "(#{x},#{y})" }.join(',')}\n" }
    File.write(File.join(dir, "a070911#{suffix}_polygons.txt"), header + polygons.join)
  end

  # n => value from a b-file (comment lines skipped).
  def read_bfile(path)
    return {} unless File.file?(path)

    File.foreach(path).filter_map do |line|
      next if line.start_with?('#')

      n, v = line.split.map(&:to_i)
      [n, v] if v
    end.to_h
  end

  def same?(label, mine, published)
    common = mine.keys & published.keys
    diff = common.reject { |n| mine[n] == published[n] }
    puts "#{label}: #{common.size} values compared with the published data, #{diff.size} differ"
    puts "  first differing n: #{diff.first(10).join(', ')}" unless diff.empty?
    diff.empty?
  end
end
