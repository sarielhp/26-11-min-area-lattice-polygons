# frozen_string_literal: true

require_relative 'common'

# Runs one modular-solver job per count, largest first, with at most `jobs`
# at once and their estimated memory below `cap_mib`. Each job may use the
# whole cap; the estimate (a k^3.5 model) is scaled by the largest observed
# ratio of measured to modeled memory.
class ModularJobs
  def initialize(dir, jobs:, cap_mib:)
    @dir = dir
    @jobs = jobs
    @cap = cap_mib
    @factor = 2.0
    @running = {}
    FileUtils.mkdir_p(dir)
  end

  def run(counts)
    queue = counts.sort.reverse
    results = {}
    until queue.empty? && @running.empty?
      launch(queue)
      count, result = finish
      results[count] = result if count
    end
    results
  ensure
    @running.each_key { |pid| Process.kill('TERM', pid) if alive?(pid) }
  end

  private

  def model(count) = 700.0 * ((count / 511.0)**3.5) * (count.odd? ? 1.0 : 0.5)

  def estimate(count) = [(model(count) * @factor).ceil, 64].max

  def file(count, ext) = File.join(@dir, format('k%<n>06d.%<e>s', n: count, e: ext))

  def launch(queue)
    while (count = queue.first) && @running.size < @jobs
      used = @running.values.sum { |job| job[:mib] }
      break if used.positive? && used + estimate(count) > @cap

      queue.shift
      pid = Process.spawn(Repro::BIN[:modular], '--k', count.to_s, '--heuristic',
                          '--memory-mib', [@cap, 65_536].min.to_s, '--witness', file(count, 'txt'),
                          out: file(count, 'json'), err: file(count, 'err'))
      @running[pid] = { count: count, mib: estimate(count) }
    end
  end

  def finish
    pid, status = Process.wait2
    job = @running.delete(pid)
    return unless job

    count = job[:count]
    abort "n=#{count}: modular solver failed: #{File.read(file(count, 'err'))}" unless status.success?
    result = JSON.parse(File.read(file(count, 'json')))
    @factor = [@factor, 1.1 * result.fetch('peak_rss_kib') / 1024.0 / model(count)].max
    gb = result['peak_rss_kib'] / 1_048_576.0
    puts "n=#{count}: doubled area #{result['doubled_area']}, #{result['search_seconds'].round(1)} s, " \
         "#{gb.round(2)} GB, #{@running.size} still running"
    [count, result]
  end

  def alive?(pid)
    Process.kill(0, pid)
    true
  rescue Errno::ESRCH
    false
  end
end
